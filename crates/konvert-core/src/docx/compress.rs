//! A smaller DOCX: pictures sharper than they are shown are scaled down and
//! stored again (JPEG stays JPEG; PNG stays PNG, with 256 colors from the
//! strong level when it has no transparency), the page thumbnail goes, and
//! every part is compressed with the strongest Deflate setting. Text and
//! layout are untouched; a picture is replaced only when it gets smaller.

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use super::{Package, rels_name, resolve_target};
use crate::error::CoreError;
use crate::pdf_compress::{Shrink, deflate, encode_jpeg, png_filter, quantize, resample, steps};
use crate::request::CompressOptions;

/// A smaller copy of a DOCX (the smallest found when there is a limit).
/// Blocking: run it on a worker thread.
pub fn compress_docx(
    data: &[u8],
    options: &CompressOptions,
    cancel: &AtomicBool,
    mut progress: impl FnMut(f64),
) -> Result<Vec<u8>, CoreError> {
    let package = Package::open(data)?;
    let shown = shown_sizes(&package);
    let steps = steps(options.level, options.max_bytes.is_some());
    let thumbnail = options.level > crate::request::CompressLevel::Light;
    let pictures: Vec<&str> = package
        .names()
        .filter(|n| n.starts_with("word/media/"))
        .collect();
    // Each picture decoded once, stored for every step.
    let mut versions: HashMap<String, Vec<Option<Vec<u8>>>> = HashMap::new();
    for (i, name) in pictures.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        let original = package.get(name).unwrap_or_default();
        if let Some(size) = shown.get(*name) {
            let stored = smaller_pictures(original, *size, &steps);
            if stored.iter().any(Option::is_some) {
                versions.insert((*name).to_owned(), stored);
            }
        }
        progress((i + 1) as f64 / (pictures.len() + 1) as f64 * 0.8);
    }
    let mut best: Option<Vec<u8>> = None;
    for step in 0..steps.len() {
        let bytes = repack(&package, &versions, step, thumbnail)?;
        let fits = options
            .max_bytes
            .is_none_or(|max| bytes.len() as u64 <= max);
        if best.as_ref().is_none_or(|b| bytes.len() < b.len()) {
            best = Some(bytes);
        }
        if fits {
            break;
        }
    }
    progress(1.0);
    Ok(best.expect("at least one step"))
}

/// The largest size, in points, at which each picture is shown (pictures of
/// the document, its headers, footers and notes).
fn shown_sizes(package: &Package) -> HashMap<String, (f32, f32)> {
    let mut sizes: HashMap<String, (f32, f32)> = HashMap::new();
    let parts: Vec<&str> = package
        .names()
        .filter(|n| n.starts_with("word/") && n.ends_with(".xml") && !n.contains("/_rels/"))
        .collect();
    for part in parts {
        let Some(text) = package.text(part) else {
            continue;
        };
        if !text.contains("embed") && !text.contains("imagedata") {
            continue;
        }
        let Ok(xml) = roxmltree::Document::parse(&text) else {
            continue;
        };
        let rels = relationship_targets(package, part);
        for frame in xml
            .descendants()
            .filter(|n| n.is_element() && matches!(n.tag_name().name(), "inline" | "anchor"))
        {
            let extent = frame
                .children()
                .find(|c| c.is_element() && c.tag_name().name() == "extent");
            let blip = frame
                .descendants()
                .find(|c| c.is_element() && c.tag_name().name() == "blip");
            let (Some(extent), Some(blip)) = (extent, blip) else {
                continue;
            };
            let number = |name: &str| {
                extent
                    .attributes()
                    .find(|a| a.name() == name)
                    .and_then(|a| a.value().parse::<f32>().ok())
                    .map(|v| v * super::read::EMU)
            };
            let id = blip
                .attributes()
                .find(|a| a.name() == "embed")
                .map(|a| a.value());
            if let (Some(w), Some(h), Some(target)) =
                (number("cx"), number("cy"), id.and_then(|id| rels.get(id)))
            {
                let entry = sizes.entry(target.clone()).or_insert((0.0, 0.0));
                entry.0 = entry.0.max(w);
                entry.1 = entry.1.max(h);
            }
        }
    }
    sizes
}

/// Relationship id → resolved part name, for one part.
fn relationship_targets(package: &Package, part: &str) -> HashMap<String, String> {
    let mut targets = HashMap::new();
    let Some(text) = package.text(&rels_name(part)) else {
        return targets;
    };
    let Ok(xml) = roxmltree::Document::parse(&text) else {
        return targets;
    };
    for rel in xml.root_element().children().filter(|c| c.is_element()) {
        let get = |name: &str| {
            rel.attributes()
                .find(|a| a.name() == name)
                .map(|a| a.value())
        };
        if get("TargetMode") == Some("External") {
            continue;
        }
        if let (Some(id), Some(target)) = (get("Id"), get("Target")) {
            targets.insert(id.to_owned(), resolve_target(part, target));
        }
    }
    targets
}

/// A picture stored again for each step, when that is at least 3% smaller.
fn smaller_pictures(original: &[u8], shown: (f32, f32), steps: &[Shrink]) -> Vec<Option<Vec<u8>>> {
    let jpeg = original.starts_with(&[0xFF, 0xD8]);
    let png = original.starts_with(&[0x89, b'P', b'N', b'G']);
    if !jpeg && !png {
        return vec![None; steps.len()];
    }
    let Ok(decoded) = image::load_from_memory(original) else {
        return vec![None; steps.len()];
    };
    let (width, height) = (decoded.width(), decoded.height());
    let alpha = decoded.color().has_alpha() && decoded.to_rgba8().pixels().any(|p| p.0[3] < 255);
    let gray = !decoded.color().has_color();
    let (pixels, channels) = if alpha {
        (decoded.to_rgba8().into_raw(), 4)
    } else if gray {
        (decoded.to_luma8().into_raw(), 1)
    } else {
        (decoded.to_rgb8().into_raw(), 3)
    };
    let limit = original.len() as f64 * 0.97;
    steps
        .iter()
        .map(|step| {
            let needed = |points: f32| ((points / 72.0) * step.dpi).ceil().max(1.0) as u32;
            let (w, h) = (needed(shown.0), needed(shown.1));
            let (w, h) = if width as f32 > w as f32 * 1.25 || height as f32 > h as f32 * 1.25 {
                (w.min(width), h.min(height))
            } else {
                (width, height)
            };
            let scaled = resample(&pixels, channels, width, height, w, h);
            let stored = if jpeg && channels != 4 {
                encode_jpeg(&scaled, channels, w, h, step.quality)
            } else if channels == 3 && step.fewer_colors {
                let (palette, indices) = quantize(&scaled, 256);
                Some(indexed_png(&indices, &palette, w, h))
            } else {
                Some(plain_png(&scaled, channels, w, h))
            };
            stored.filter(|s| (s.len() as f64) < limit)
        })
        .collect()
}

/// The package again with the pictures of `step`, without the page thumbnail
/// when asked, every part compressed at the strongest level.
fn repack(
    package: &Package,
    versions: &HashMap<String, Vec<Option<Vec<u8>>>>,
    step: usize,
    drop_thumbnail: bool,
) -> Result<Vec<u8>, CoreError> {
    let io = |e: zip::result::ZipError| CoreError::Io(std::io::Error::other(e.to_string()));
    let thumbnail = |name: &str| name.starts_with("docProps/thumbnail");
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let deflated = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(9));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for name in package.names() {
        if drop_thumbnail && thumbnail(name) {
            continue;
        }
        let original = package.get(name).unwrap_or_default();
        let replaced = versions
            .get(name)
            .and_then(|v| v.get(step))
            .and_then(Option::as_ref);
        let data: std::borrow::Cow<[u8]> = match replaced {
            Some(smaller) => smaller.as_slice().into(),
            None if drop_thumbnail && name == "_rels/.rels" => {
                without_thumbnail(&String::from_utf8_lossy(original))
                    .into_bytes()
                    .into()
            }
            None => original.into(),
        };
        // Pictures are compressed already: Deflate would only cost time.
        let picture = name.starts_with("word/media/");
        zip.start_file(name, if picture { stored } else { deflated })
            .map_err(io)?;
        zip.write_all(&data)?;
    }
    Ok(zip.finish().map_err(io)?.into_inner())
}

/// The package relationships without the one to the page thumbnail.
fn without_thumbnail(rels: &str) -> String {
    let mut out = String::with_capacity(rels.len());
    let mut rest = rels;
    while let Some(start) = rest.find("<Relationship ") {
        let Some(len) = rest[start..].find("/>") else {
            break;
        };
        let element = &rest[start..start + len + 2];
        out.push_str(&rest[..start]);
        if !element.contains("thumbnail") {
            out.push_str(element);
        }
        rest = &rest[start + len + 2..];
    }
    out.push_str(rest);
    out
}

/// A PNG of gray (1), RGB (3) or RGBA (4) pixels.
pub(crate) fn plain_png(pixels: &[u8], channels: usize, width: u32, height: u32) -> Vec<u8> {
    let color_type = match channels {
        1 => 0,
        4 => 6,
        _ => 2,
    };
    let filtered = png_filter(pixels, channels, width as usize);
    png(width, height, color_type, None, &deflate(&filtered))
}

/// An 8-bit palette PNG.
fn indexed_png(indices: &[u8], palette: &[[u8; 3]], width: u32, height: u32) -> Vec<u8> {
    let stride = width as usize;
    let mut rows = Vec::with_capacity((stride + 1) * height as usize);
    for row in indices.chunks(stride.max(1)) {
        rows.push(0);
        rows.extend_from_slice(row);
    }
    let table: Vec<u8> = palette.iter().flatten().copied().collect();
    png(width, height, 3, Some(&table), &deflate(&rows))
}

fn png(width: u32, height: u32, color_type: u8, palette: Option<&[u8]>, idat: &[u8]) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut header = Vec::with_capacity(13);
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    header.extend([8, color_type, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    if let Some(palette) = palette {
        chunk(&mut out, b"PLTE", palette);
    }
    chunk(&mut out, b"IDAT", idat);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    out.extend(kind);
    out.extend(data);
    let mut crc = crc32(kind, 0xFFFF_FFFF);
    crc = crc32(data, crc);
    out.extend((crc ^ 0xFFFF_FFFF).to_be_bytes());
}

fn crc32(data: &[u8], mut crc: u32) -> u32 {
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                0xEDB8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pngs_written_here_decode() {
        let rgb: Vec<u8> = (0..16 * 8)
            .flat_map(|i| [(i * 2) as u8, 100, (255 - i) as u8])
            .collect();
        let png = plain_png(&rgb, 3, 16, 8);
        let decoded = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(decoded.into_raw(), rgb);
        let (palette, indices) = quantize(&rgb, 256);
        let png = indexed_png(&indices, &palette, 16, 8);
        let decoded = image::load_from_memory(&png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (16, 8));
        assert_eq!(crc32(b"IEND", 0xFFFF_FFFF) ^ 0xFFFF_FFFF, 0xAE42_6082);
    }

    #[test]
    fn the_thumbnail_relationship_goes() {
        let rels = "<Relationships><Relationship Id=\"rId1\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Target=\"docProps/thumbnail.jpeg\"/></Relationships>";
        let out = without_thumbnail(rels);
        assert!(out.contains("word/document.xml") && !out.contains("thumbnail"));
        assert!(out.ends_with("</Relationships>"));
    }
}
