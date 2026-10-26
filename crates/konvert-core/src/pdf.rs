//! PDF documents, handled without FFmpeg:
//!
//! * analysis ([`probe`]) and rendering of pages to pictures with hayro (pure
//!   Rust, no unsafe code);
//! * pictures laid out on pages ([`place`]) and documents assembled or merged
//!   ([`assemble`]) with lopdf.
//!
//! Konvert never asks for passwords: a PDF that needs one gives
//! [`CoreError::PdfEncrypted`]. PDFs protected only against editing (empty
//! user password) open normally. The PDFs Konvert writes carry no path, user
//! name or date: their only metadata is `Producer: Konvert`.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::{LoadPdfError, Pdf};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::CoreError;
use crate::probe::{MediaInfo, MediaKind};
use crate::request::{PageMargin, PageOrientation, PageSize, PdfOptions};

/// A4 in points (1/72 inch).
pub const A4: (f32, f32) = (595.276, 841.89);
/// US Letter in points.
pub const LETTER: (f32, f32) = (612.0, 792.0);
/// Highest rendering resolution offered.
pub const MAX_DPI: u32 = 600;
/// Largest rendered page, in pixels (a 100 MP page is 400 MB of RGBA).
const MAX_PAGE_PIXELS: f64 = 100_000_000.0;

/// What Konvert knows about a PDF document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DocumentInfo {
    pub pages: u32,
    /// Size of the first page, in points (rotation applied).
    pub width_pt: f32,
    pub height_pt: f32,
}

/// The file starts like a PDF (`%PDF-` within its first kilobyte).
pub fn is_pdf(path: &Path) -> bool {
    use std::io::Read;
    let mut head = Vec::with_capacity(1024);
    std::fs::File::open(path)
        .and_then(|f| f.take(1024).read_to_end(&mut head))
        .is_ok()
        && head.windows(5).any(|w| w == b"%PDF-")
}

/// Opens a PDF with hayro, mapping its errors to Konvert's.
pub(crate) fn open(data: Vec<u8>) -> Result<Pdf, CoreError> {
    match Pdf::new(data) {
        Ok(pdf) if pdf.pages().is_empty() => Err(damaged("the document has no pages")),
        Ok(pdf) => Ok(pdf),
        Err(LoadPdfError::Decryption(e)) => match e {
            hayro::hayro_syntax::DecryptionError::PasswordProtected => Err(CoreError::PdfEncrypted),
            other => Err(damaged(&format!("encryption: {other:?}"))),
        },
        Err(LoadPdfError::Invalid) => Err(damaged("not a valid PDF file")),
    }
}

fn damaged(details: &str) -> CoreError {
    CoreError::CorruptInput {
        details: format!("PDF: {details}"),
    }
}

/// Analyses a PDF file: a document with its page count and first page size.
pub fn probe(path: &Path) -> Result<MediaInfo, CoreError> {
    let data = std::fs::read(path)?;
    let size = data.len() as u64;
    let pdf =
        std::panic::catch_unwind(|| open(data)).map_err(|_| damaged("the reader failed"))??;
    let pages = pdf.pages();
    let (width_pt, height_pt) = pages[0].render_dimensions();
    Ok(MediaInfo {
        container: "pdf".into(),
        kind: MediaKind::Document,
        duration_secs: None,
        size_bytes: Some(size),
        bit_rate: None,
        audio: Vec::new(),
        video: Vec::new(),
        cover_art: None,
        subtitle_streams: 0,
        picture: None,
        has_tags: false,
        has_location: false,
        document: Some(DocumentInfo {
            pages: pages.len() as u32,
            width_pt,
            height_pt,
        }),
    })
}

/// Pages chosen by the user, 1-based and in document order: `None` or an
/// empty text means every page; otherwise `"1-3, 5, 8-"` (open ranges allowed).
pub fn parse_pages(text: Option<&str>, count: u32) -> Result<Vec<u32>, CoreError> {
    let invalid = |part: &str| CoreError::InvalidPages {
        details: format!("invalid page range \"{part}\" (the document has {count} pages)"),
    };
    let text = text.map(str::trim).unwrap_or_default();
    if text.is_empty() {
        return Ok((1..=count).collect());
    }
    let mut chosen = vec![false; count as usize + 1];
    for part in text
        .split([',', ';'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        let number = |s: &str| s.trim().parse::<u32>().map_err(|_| invalid(part));
        let (first, last) = match part.split_once('-') {
            Some((a, b)) => (
                if a.trim().is_empty() { 1 } else { number(a)? },
                if b.trim().is_empty() {
                    count
                } else {
                    number(b)?
                },
            ),
            None => (number(part)?, number(part)?),
        };
        if first == 0 || first > last || last > count {
            return Err(invalid(part));
        }
        for page in first..=last {
            chosen[page as usize] = true;
        }
    }
    Ok((1..=count).filter(|p| chosen[*p as usize]).collect())
}

/// Scale factor to render a page of `width_pt`×`height_pt` at `dpi`, reduced if
/// the page would exceed the pixel budget.
pub fn render_scale(width_pt: f32, height_pt: f32, dpi: u32) -> f32 {
    let scale = dpi.clamp(36, MAX_DPI) as f32 / 72.0;
    let pixels = (width_pt * scale) as f64 * (height_pt * scale) as f64;
    if pixels > MAX_PAGE_PIXELS {
        scale * (MAX_PAGE_PIXELS / pixels).sqrt() as f32
    } else {
        scale
    }
}

/// Size in pixels of a page rendered at `dpi`.
pub fn rendered_size(width_pt: f32, height_pt: f32, dpi: u32) -> (u32, u32) {
    let scale = render_scale(width_pt, height_pt, dpi);
    (
        ((width_pt * scale) as u32).max(1),
        ((height_pt * scale) as u32).max(1),
    )
}

/// One rendered page, as a PNG file.
pub struct RenderedPage {
    /// 1-based page number.
    pub number: u32,
    pub png: Vec<u8>,
}

/// Renders the given pages on a white background, calling `on_page` after
/// each one; stops early when `cancel` is set or `on_page` returns `false`.
/// Blocking: run it on a worker thread.
pub fn render_pages(
    data: Vec<u8>,
    pages: &[u32],
    dpi: u32,
    cancel: &AtomicBool,
    mut on_page: impl FnMut(RenderedPage) -> bool,
) -> Result<(), CoreError> {
    let pdf = open(data)?;
    let all = pdf.pages();
    let cache = RenderCache::new();
    let settings = InterpreterSettings::default();
    for &number in pages {
        if cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        let page = all
            .get(number as usize - 1)
            .ok_or_else(|| damaged(&format!("page {number} is missing")))?;
        let (width, height) = page.render_dimensions();
        let scale = render_scale(width, height, dpi);
        // A renderer bug on one strange page must not take the app down.
        let pixmap = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            hayro::render(
                page,
                &cache,
                &settings,
                &RenderSettings::default(),
                &PixmapSettings {
                    x_scale: scale,
                    y_scale: scale,
                    bg_color: WHITE,
                },
            )
        }))
        .map_err(|_| damaged(&format!("page {number} could not be drawn")))?;
        let png = pixmap
            .into_png()
            .map_err(|e| CoreError::Io(std::io::Error::other(e.to_string())))?;
        if !on_page(RenderedPage { number, png }) {
            return Err(CoreError::Cancelled);
        }
    }
    Ok(())
}

// --- Pictures on pages -------------------------------------------------------------

/// Where a picture goes on its page, in points (origin at the bottom left).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Placement {
    pub page_width: f32,
    pub page_height: f32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Placement {
    /// Longest side of the picture on paper, in inches.
    pub fn longest_inches(&self) -> f32 {
        self.width.max(self.height) / 72.0
    }
}

fn margin_points(margin: PageMargin) -> f32 {
    match margin {
        PageMargin::None => 0.0,
        // About 0.5 cm and 1.5 cm.
        PageMargin::Small => 14.0,
        PageMargin::Normal => 42.0,
    }
}

/// Lays out a `width`×`height` picture on a page: A4 or Letter (turned to
/// match the picture unless an orientation is forced), or a page shaped
/// like the picture whose long side is as long as A4's. The picture is as
/// large as the margins allow, centered.
pub fn place(width: u32, height: u32, options: &PdfOptions) -> Placement {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let margin = margin_points(options.margin);
    let landscape = match options.orientation {
        PageOrientation::Auto => w > h,
        PageOrientation::Portrait => false,
        PageOrientation::Landscape => true,
    };
    let (page_width, page_height) = match options.page_size {
        PageSize::Fit => {
            let long = A4.1;
            let (iw, ih) = if w >= h {
                (long, long * h / w)
            } else {
                (long * w / h, long)
            };
            (iw + 2.0 * margin, ih + 2.0 * margin)
        }
        size => {
            let (short, long) = if size == PageSize::Letter { LETTER } else { A4 };
            if landscape {
                (long, short)
            } else {
                (short, long)
            }
        }
    };
    let (area_w, area_h) = (page_width - 2.0 * margin, page_height - 2.0 * margin);
    let scale = (area_w / w).min(area_h / h);
    let (width, height) = (w * scale, h * scale);
    Placement {
        page_width,
        page_height,
        x: (page_width - width) / 2.0,
        y: (page_height - height) / 2.0,
        width,
        height,
    }
}

// --- Assembling documents ------------------------------------------------------------

/// One part of an assembled document.
pub enum Part {
    /// A JPEG picture (RGB) of `width`×`height` pixels on its own page.
    Picture {
        jpeg: Vec<u8>,
        width: u32,
        height: u32,
        placement: Placement,
    },
    /// Every page of a PDF file.
    Document(Vec<u8>),
}

/// What was left out while assembling.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AssembleReport {
    pub pages: u32,
    /// Bookmarks or form fields of the merged documents were not kept.
    pub dropped_extras: bool,
}

/// Builds one PDF from pictures and documents, in order.
pub fn assemble(parts: Vec<Part>) -> Result<(Vec<u8>, AssembleReport), CoreError> {
    let mut out = Document::with_version("1.7");
    let pages_id = out.new_object_id();
    let mut kids: Vec<Object> = Vec::new();
    let mut report = AssembleReport::default();

    for part in parts {
        match part {
            Part::Picture {
                jpeg,
                width,
                height,
                placement,
            } => {
                let image = Stream::new(
                    dictionary! {
                        "Type" => "XObject",
                        "Subtype" => "Image",
                        "Width" => width as i64,
                        "Height" => height as i64,
                        "ColorSpace" => "DeviceRGB",
                        "BitsPerComponent" => 8,
                        "Filter" => "DCTDecode",
                    },
                    jpeg,
                );
                let image_id = out.add_object(image);
                let content = format!(
                    "q {:.3} 0 0 {:.3} {:.3} {:.3} cm /Im0 Do Q",
                    placement.width, placement.height, placement.x, placement.y
                );
                let mut content = Stream::new(Dictionary::new(), content.into_bytes());
                let _ = content.compress();
                let content_id = out.add_object(content);
                let page = dictionary! {
                    "Type" => "Page",
                    "Parent" => pages_id,
                    "MediaBox" => vec![0.into(), 0.into(), placement.page_width.into(), placement.page_height.into()],
                    "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
                    "Contents" => content_id,
                };
                kids.push(out.add_object(page).into());
                report.pages += 1;
            }
            Part::Document(data) => {
                let (pages, dropped) = append_document(&mut out, pages_id, &data)?;
                report.pages += pages.len() as u32;
                report.dropped_extras |= dropped;
                kids.extend(pages.into_iter().map(Object::from));
            }
        }
    }

    let count = kids.len() as i64;
    out.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
    );
    let catalog_id = out.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    let info_id = out.add_object(dictionary! { "Producer" => Object::string_literal("Konvert") });
    out.trailer.set("Root", catalog_id);
    out.trailer.set("Info", info_id);
    out.compress();
    let mut bytes = Vec::new();
    out.save_to(&mut bytes)
        .map_err(|e| CoreError::Io(std::io::Error::other(e.to_string())))?;
    Ok((bytes, report))
}

/// Page attributes a page may inherit from its parents.
const INHERITED: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// Copies every page of `data` into `out` (under `parent`) and returns the
/// new page ids. Only the pages and what they use are kept: the document
/// catalog, outlines, form and metadata are not.
fn append_document(
    out: &mut Document,
    parent: ObjectId,
    data: &[u8],
) -> Result<(Vec<ObjectId>, bool), CoreError> {
    // hayro checks the file first: it tells a password-protected PDF apart.
    open(data.to_vec())?;
    let mut doc = Document::load_mem(data).map_err(|e| match e {
        lopdf::Error::Decryption(_) => CoreError::PdfEncrypted,
        other => damaged(&other.to_string()),
    })?;
    if doc.is_encrypted() {
        doc.decrypt("").map_err(|_| CoreError::PdfEncrypted)?;
    }
    let dropped = doc
        .catalog()
        .is_ok_and(|c| c.has(b"Outlines") || c.has(b"AcroForm"));

    doc.renumber_objects_with(out.max_id + 1);
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    if pages.is_empty() {
        return Err(damaged("the document has no pages"));
    }
    // Inherited attributes become the page's own, since the page tree changes.
    for &page_id in &pages {
        let mut inherited: Vec<(&[u8], Object)> = Vec::new();
        let mut node = doc.get_dictionary(page_id).ok().cloned();
        let mut depth = 0;
        while let Some(dict) = node {
            for key in INHERITED {
                if !inherited.iter().any(|(k, _)| *k == key)
                    && let Ok(value) = dict.get(key)
                {
                    inherited.push((key, value.clone()));
                }
            }
            depth += 1;
            node = match dict.get(b"Parent").and_then(Object::as_reference) {
                Ok(parent) if depth < 64 => doc.get_dictionary(parent).ok().cloned(),
                _ => None,
            };
        }
        let page = doc
            .get_dictionary_mut(page_id)
            .map_err(|e| damaged(&e.to_string()))?;
        for (key, value) in inherited {
            page.set(key, value);
        }
        page.set("Parent", parent);
    }

    // Keep everything except the document-level structure.
    let skip = |object: &Object| {
        object.as_dict().is_ok_and(|d| {
            matches!(
                d.get(b"Type").and_then(Object::as_name),
                Ok(b"Catalog" | b"Pages" | b"Outlines" | b"Outline" | b"Metadata")
            )
        })
    };
    let info = doc.trailer.get(b"Info").and_then(Object::as_reference).ok();
    for (id, object) in doc.objects {
        if skip(&object) || Some(id) == info {
            continue;
        }
        out.objects.insert(id, object);
    }
    out.max_id = out
        .objects
        .keys()
        .map(|(id, _)| *id)
        .max()
        .unwrap_or(out.max_id);
    Ok((pages, dropped))
}

/// Number of pages of a PDF file (for tests and checks).
pub fn page_count(data: &[u8]) -> Result<u32, CoreError> {
    Ok(open(data.to_vec())?.pages().len() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PDF with one page per size (points), each with a filled rectangle.
    pub(crate) fn sample_pdf(sizes: &[(f32, f32)]) -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let mut kids = Vec::new();
        for (i, (w, h)) in sizes.iter().enumerate() {
            let shade = 0.2 + 0.1 * i as f32;
            let content = format!("{shade} 0 0 rg 10 10 {} {} re f", w - 20.0, h - 20.0);
            let content_id = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
            let page = dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), (*w).into(), (*h).into()],
                "Contents" => content_id,
            };
            kids.push(Object::from(doc.add_object(page)));
        }
        let count = kids.len() as i64;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        let info = doc.add_object(dictionary! {
            "Author" => Object::string_literal("Mario Rossi"),
            "CreationDate" => Object::string_literal("D:20260101120000"),
        });
        doc.trailer.set("Root", catalog);
        doc.trailer.set("Info", info);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn page_ranges() {
        assert_eq!(parse_pages(None, 3).unwrap(), [1, 2, 3]);
        assert_eq!(parse_pages(Some("  "), 2).unwrap(), [1, 2]);
        assert_eq!(parse_pages(Some("3, 1-2"), 5).unwrap(), [1, 2, 3]);
        assert_eq!(parse_pages(Some("4-; 2"), 6).unwrap(), [2, 4, 5, 6]);
        assert_eq!(parse_pages(Some("-2"), 6).unwrap(), [1, 2]);
        for wrong in ["0", "7", "3-2", "a", "1-x", "2--3"] {
            let err = parse_pages(Some(wrong), 6).unwrap_err();
            assert_eq!(err.code(), "invalid_pages", "{wrong}");
        }
    }

    #[test]
    fn pictures_fill_the_page_inside_the_margins() {
        let options = PdfOptions::default();
        // A landscape photo on A4: the page turns, small margins.
        let p = place(4000, 3000, &options);
        assert_eq!((p.page_width, p.page_height), (A4.1, A4.0));
        assert!((p.height - (A4.0 - 28.0)).abs() < 0.01);
        assert!((p.x * 2.0 + p.width - p.page_width).abs() < 0.01);
        // Forced portrait.
        let portrait = PdfOptions {
            orientation: PageOrientation::Portrait,
            margin: PageMargin::None,
            ..options.clone()
        };
        let p = place(4000, 3000, &portrait);
        assert_eq!(
            (p.page_width, p.page_height, p.x, p.width),
            (A4.0, A4.1, 0.0, A4.0)
        );
        // A page shaped like the picture, long side as long as A4.
        let fit = PdfOptions {
            page_size: PageSize::Fit,
            margin: PageMargin::None,
            ..options.clone()
        };
        let p = place(1000, 2000, &fit);
        assert_eq!((p.page_width, p.page_height), (A4.1 / 2.0, A4.1));
        assert_eq!((p.x, p.y), (0.0, 0.0));
        let letter = PdfOptions {
            page_size: PageSize::Letter,
            ..options
        };
        assert_eq!(place(10, 20, &letter).page_width, LETTER.0);
    }

    #[test]
    fn huge_pages_are_rendered_smaller() {
        assert_eq!(rendered_size(A4.0, A4.1, 150), (1240, 1753));
        // A 2 m × 2 m poster at 600 DPI stays within the pixel budget.
        let (w, h) = rendered_size(5669.0, 5669.0, 600);
        assert!((w as f64 * h as f64) <= MAX_PAGE_PIXELS + 1.0);
    }

    #[test]
    fn documents_are_merged_page_by_page_without_personal_metadata() {
        let first = sample_pdf(&[(A4.0, A4.1), (A4.1, A4.0)]);
        let second = sample_pdf(&[(LETTER.0, LETTER.1)]);
        let (merged, report) =
            assemble(vec![Part::Document(first), Part::Document(second)]).unwrap();
        assert_eq!(report.pages, 3);
        assert!(!report.dropped_extras);
        assert_eq!(page_count(&merged).unwrap(), 3);
        // Sizes and order are kept.
        let pdf = open(merged.clone()).unwrap();
        let sizes: Vec<(f32, f32)> = pdf.pages().iter().map(|p| p.render_dimensions()).collect();
        assert_eq!(sizes, [(A4.0, A4.1), (A4.1, A4.0), (LETTER.0, LETTER.1)]);
        // The author and the date of the sources are not carried over.
        let text = String::from_utf8_lossy(&merged);
        assert!(!text.contains("Mario Rossi") && !text.contains("D:2026"));
        let doc = Document::load_mem(&merged).unwrap();
        let info = doc
            .trailer
            .get(b"Info")
            .and_then(Object::as_reference)
            .unwrap();
        let info = doc.get_dictionary(info).unwrap();
        assert_eq!(info.get(b"Producer").unwrap().as_str().unwrap(), b"Konvert");
        assert_eq!(info.len(), 1);
    }

    #[test]
    fn password_protected_and_damaged_pdfs_are_told_apart() {
        use lopdf::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter};
        use lopdf::{EncryptionState, EncryptionVersion, Permissions};
        use std::collections::BTreeMap;
        use std::sync::Arc;

        let encrypt = |user: &str| {
            let mut doc = Document::load_mem(&sample_pdf(&[(A4.0, A4.1)])).unwrap();
            doc.trailer.set(
                "ID",
                vec![
                    Object::string_literal("0123456789abcdef"),
                    Object::string_literal("0123456789abcdef"),
                ],
            );
            let filter: Arc<dyn CryptFilter> = Arc::new(Aes128CryptFilter);
            let version = EncryptionVersion::V4 {
                document: &doc,
                encrypt_metadata: true,
                crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), filter)]),
                stream_filter: b"StdCF".to_vec(),
                string_filter: b"StdCF".to_vec(),
                owner_password: "proprietario",
                user_password: user,
                permissions: Permissions::all(),
            };
            let state = EncryptionState::try_from(version).unwrap();
            doc.encrypt(&state).unwrap();
            let mut bytes = Vec::new();
            doc.save_to(&mut bytes).unwrap();
            bytes
        };
        // A password is needed: a clear, dedicated error.
        let locked = encrypt("segreta");
        assert_eq!(open(locked.clone()).err().unwrap().code(), "pdf_encrypted");
        assert_eq!(
            assemble(vec![Part::Document(locked)]).unwrap_err().code(),
            "pdf_encrypted"
        );
        // Only editing is restricted: it opens, renders and merges.
        let restricted = encrypt("");
        assert_eq!(page_count(&restricted).unwrap(), 1);
        let (merged, _) = assemble(vec![Part::Document(restricted)]).unwrap();
        assert_eq!(page_count(&merged).unwrap(), 1);

        // Damaged files.
        let mut truncated = sample_pdf(&[(A4.0, A4.1)]);
        truncated.truncate(truncated.len() / 3);
        for data in [b"%PDF-1.7 garbage".to_vec(), Vec::new()] {
            assert_eq!(open(data).err().unwrap().code(), "corrupt_input");
        }
        // A truncated file either opens what it can or fails cleanly, never panics.
        let _ = open(truncated);
    }

    #[test]
    fn pages_render_at_the_requested_resolution_and_can_be_cancelled() {
        let data = sample_pdf(&[(A4.0, A4.1), (200.0, 100.0), (A4.0, A4.1)]);
        let mut seen = Vec::new();
        render_pages(
            data.clone(),
            &[2, 3],
            144,
            &AtomicBool::new(false),
            |page| {
                seen.push((page.number, page.png.len() > 100));
                true
            },
        )
        .unwrap();
        assert_eq!(seen, [(2, true), (3, true)]);
        // The second page is 200×100 points: 400×200 pixels at 144 DPI.
        let mut size = None;
        render_pages(data.clone(), &[2], 144, &AtomicBool::new(false), |page| {
            let header = &page.png[16..24];
            size = Some((
                u32::from_be_bytes(header[..4].try_into().unwrap()),
                u32::from_be_bytes(header[4..].try_into().unwrap()),
            ));
            true
        })
        .unwrap();
        assert_eq!(size, Some((400, 200)));
        // Cancelled before the first page.
        let err = render_pages(data, &[1], 72, &AtomicBool::new(true), |_| true).unwrap_err();
        assert_eq!(err.code(), "cancelled");
    }

    #[test]
    fn pictures_become_pages() {
        // A tiny but valid JPEG is not needed to assemble: the bytes are copied.
        let placement = place(400, 300, &PdfOptions::default());
        let (pdf, report) = assemble(vec![
            Part::Picture {
                jpeg: vec![0xff, 0xd8, 0xff, 0xd9],
                width: 400,
                height: 300,
                placement,
            },
            Part::Document(sample_pdf(&[(A4.0, A4.1)])),
        ])
        .unwrap();
        assert_eq!(report.pages, 2);
        let doc = Document::load_mem(&pdf).unwrap();
        assert_eq!(doc.get_pages().len(), 2);
        assert!(is_pdf_bytes(&pdf));
    }

    fn is_pdf_bytes(data: &[u8]) -> bool {
        data.starts_with(b"%PDF-")
    }
}
