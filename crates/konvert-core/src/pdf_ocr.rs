//! Searchable PDFs: pages without any text (scans, photos of documents) are
//! read by OCR ([`crate::ocr`]) and get invisible text over their picture, so
//! that their words can be found, selected and copied. The pages look the
//! same.
//!
//! Each page is drawn at 300 dpi (at most 4200 pixels on its long side) and
//! read. Every line becomes invisible text (rendering mode 3) along the line,
//! as high as the line and stretched to its length, in a font without glyphs
//! whose character codes are the Unicode text (Identity-H, with an identity
//! ToUnicode map), so copied text is exactly what was read. The lines are
//! placed in the page's own coordinates: turning the page later keeps them on
//! their words.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use hayro::RenderCache;
use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps,
    InterpreterCache, InterpreterSettings, SoftMask, TransformExt, interpret_page,
};
use hayro::hayro_syntax::page::Page;
use hayro::kurbo;
use hayro::vello_cpu::Pixmap;
use image::GrayImage;
use lopdf::{Document, Object, ObjectId, Stream, dictionary};

use crate::error::CoreError;
use crate::ocr::{Ocr, TextLine};
use crate::pdf::{self, Part, Placement};
use crate::request::PdfPage;

/// Resolution at which pages are read.
pub const OCR_DPI: f32 = 300.0;
/// Longest side of a page or picture read, in pixels.
pub(crate) const MAX_SIDE: f32 = 4200.0;
/// Name of the text's font in the pages' resources.
pub(crate) const FONT_NAME: &str = "KonvertOCR";
/// Advance of every character, in thousandths of the font size.
const GLYPH_WIDTH: i64 = 500;

/// A line of text read by OCR, in the coordinates of its page (points, y
/// upwards).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlacedLine {
    pub text: String,
    /// Start of the line, at the bottom of its box.
    pub origin: (f32, f32),
    /// Directions of the text and towards its top (unit vectors).
    pub along: (f32, f32),
    pub up: (f32, f32),
    pub length: f32,
    pub height: f32,
}

/// Text read on the pages of new documents that had none, by file and page.
#[derive(Debug, Clone, Default)]
pub struct TextLayer {
    lines: HashMap<(u32, u32), Vec<PlacedLine>>,
    /// Pages read by OCR (with or without text found).
    pub pages_read: u32,
}

impl TextLayer {
    /// The lines read on page `number` of file `file`.
    pub(crate) fn lines(&self, file: u32, number: u32) -> &[PlacedLine] {
        self.lines.get(&(file, number)).map_or(&[], Vec::as_slice)
    }

    /// Lines of text found, on every page.
    pub fn line_count(&self) -> usize {
        self.lines.values().map(Vec::len).sum()
    }

    #[cfg(test)]
    pub(crate) fn insert(&mut self, file: u32, number: u32, lines: Vec<PlacedLine>) {
        self.pages_read += 1;
        self.lines.insert((file, number), lines);
    }
}

/// Scale (pixels per point) at which a page of `width`×`height` points is
/// read.
pub(crate) fn ocr_scale(width: f32, height: f32) -> f32 {
    (OCR_DPI / 72.0).min(MAX_SIDE / width.max(height).max(1.0))
}

/// A page drawn on white, in gray.
pub(crate) fn gray(pixmap: &Pixmap) -> GrayImage {
    let data = pixmap.data_as_u8_slice();
    GrayImage::from_fn(
        u32::from(pixmap.width()),
        u32::from(pixmap.height()),
        |x, y| {
            let i = 4 * (y as usize * usize::from(pixmap.width()) + x as usize);
            let [r, g, b] = [data[i], data[i + 1], data[i + 2]].map(u32::from);
            image::Luma([((r * 299 + g * 587 + b * 114) / 1000) as u8])
        },
    )
}

/// A picture in gray, its transparent parts on white.
pub(crate) fn on_white(picture: &image::DynamicImage) -> GrayImage {
    let rgba = picture.to_rgba8();
    GrayImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0.map(u32::from);
        let luma = (r * 299 + g * 587 + b * 114) / 1000;
        image::Luma([((luma * a + 255 * (255 - a)) / 255) as u8])
    })
}

/// Reads the pages of `files` chosen by `layout` (`None`: all of them) that
/// have no text, and the pictures. `progress` gets the fraction of pages
/// done. Blocking: run it on a worker thread.
pub fn read_pages(
    files: &[Part],
    layout: Option<&[PdfPage]>,
    ocr: &Ocr,
    cancel: &AtomicBool,
    mut progress: impl FnMut(f64),
) -> Result<TextLayer, CoreError> {
    let documents = files
        .iter()
        .map(|file| match file {
            Part::Document(data) => pdf::open(data.clone()).map(Some),
            Part::Picture { .. } => Ok(None),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let counts: Vec<u32> = documents
        .iter()
        .map(|d| d.as_ref().map_or(1, |d| d.pages().len() as u32))
        .collect();
    let mut wanted: Vec<(u32, u32)> = pdf::check_pages(layout, &counts)?
        .iter()
        .map(|p| (p.file, p.number))
        .collect();
    wanted.sort_unstable();
    wanted.dedup();

    let mut layer = TextLayer::default();
    let cache = RenderCache::new();
    let settings = InterpreterSettings::default();
    for (done, &(file, number)) in wanted.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        let read = match (&files[file as usize], &documents[file as usize]) {
            (
                Part::Picture {
                    jpeg, placement, ..
                },
                _,
            ) => read_picture(jpeg, *placement, ocr, cancel)?,
            (Part::Document(_), Some(document)) => {
                let pages = document.pages();
                match pages.get(number as usize - 1) {
                    Some(page) if !has_text(page) => {
                        read_page(page, &cache, &settings, ocr, cancel)?
                    }
                    _ => None,
                }
            }
            (Part::Document(_), None) => None,
        };
        if let Some(lines) = read {
            layer.pages_read += 1;
            if !lines.is_empty() {
                layer.lines.insert((file, number), lines);
            }
        }
        progress((done + 1) as f64 / wanted.len() as f64);
    }
    Ok(layer)
}

/// Reads a page drawn at the OCR resolution; `None` when it cannot be drawn.
fn read_page<'a>(
    page: &'a Page<'a>,
    cache: &RenderCache<'a>,
    settings: &InterpreterSettings,
    ocr: &Ocr,
    cancel: &AtomicBool,
) -> Result<Option<Vec<PlacedLine>>, CoreError> {
    let (width, height) = page.render_dimensions();
    let scale = ocr_scale(width, height);
    let Some(pixmap) = pdf::draw(page, cache, settings, scale) else {
        return Ok(None);
    };
    let lines = ocr.read(&gray(&pixmap), cancel)?;
    // Pixels → points of the page as shown → the page's own coordinates.
    let to_page = page.initial_transform(true).to_kurbo().inverse()
        * kurbo::Affine::scale(1.0 / f64::from(scale));
    Ok(Some(place(&lines, to_page)))
}

/// Reads a picture of a new page; `None` when it cannot be decoded.
fn read_picture(
    jpeg: &[u8],
    placement: Placement,
    ocr: &Ocr,
    cancel: &AtomicBool,
) -> Result<Option<Vec<PlacedLine>>, CoreError> {
    let Ok(decoded) = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg) else {
        return Ok(None);
    };
    let mut picture = decoded.to_luma8();
    let longest = picture.width().max(picture.height()) as f32;
    if longest > MAX_SIDE {
        let scale = MAX_SIDE / longest;
        picture = image::imageops::resize(
            &picture,
            ((picture.width() as f32 * scale) as u32).max(1),
            ((picture.height() as f32 * scale) as u32).max(1),
            image::imageops::FilterType::Triangle,
        );
    }
    let lines = ocr.read(&picture, cancel)?;
    // Pixels → the picture's place on its page.
    let (w, h) = (f64::from(picture.width()), f64::from(picture.height()));
    let to_page = kurbo::Affine::new([
        f64::from(placement.width) / w,
        0.0,
        0.0,
        -f64::from(placement.height) / h,
        f64::from(placement.x),
        f64::from(placement.y + placement.height),
    ]);
    Ok(Some(place(&lines, to_page)))
}

/// Lines read in a picture, placed on the page by `to_page`.
pub(crate) fn place(lines: &[TextLine], to_page: kurbo::Affine) -> Vec<PlacedLine> {
    lines
        .iter()
        .filter_map(|line| {
            let [top_start, bottom_start, bottom_end, _] = line
                .corners()
                .map(|(x, y)| to_page * kurbo::Point::new(f64::from(x), f64::from(y)));
            let (along, up) = (bottom_end - bottom_start, top_start - bottom_start);
            let (length, height) = (along.hypot(), up.hypot());
            (length >= 1.0 && height >= 1.0).then(|| PlacedLine {
                text: line.text.clone(),
                origin: (bottom_start.x as f32, bottom_start.y as f32),
                along: ((along.x / length) as f32, (along.y / length) as f32),
                up: ((up.x / height) as f32, (up.y / height) as f32),
                length: length as f32,
                height: height as f32,
            })
        })
        .collect()
}

/// Content drawing `lines` as invisible text in the font [`FONT_NAME`].
pub(crate) fn drawing(lines: &[PlacedLine]) -> String {
    let mut out = String::from("BT\n3 Tr\n");
    for line in lines {
        let codes: Vec<u16> = line
            .text
            .chars()
            .filter_map(|c| u16::try_from(u32::from(c)).ok())
            .collect();
        if codes.is_empty() {
            continue;
        }
        let size = line.height;
        let natural = codes.len() as f32 * size * GLYPH_WIDTH as f32 / 1000.0;
        let stretch = 100.0 * line.length / natural;
        let hex: String = codes.iter().map(|c| format!("{c:04X}")).collect();
        let ((a, b), (c, d), (e, f)) = (line.along, line.up, line.origin);
        out.push_str(&format!(
            "/{FONT_NAME} {size:.2} Tf {stretch:.3} Tz {a:.5} {b:.5} {c:.5} {d:.5} {e:.3} {f:.3} Tm <{hex}> Tj\n"
        ));
    }
    out.push_str("ET\n");
    out
}

/// Adds the font of the invisible text to `doc`; gives the font.
pub(crate) fn add_font(doc: &mut Document) -> ObjectId {
    let program = glyphless_font();
    let mut file = Stream::new(dictionary! { "Length1" => program.len() as i64 }, program);
    let _ = file.compress();
    let file = doc.add_object(file);
    // Every character code shows glyph 1, which draws nothing.
    let mut map = Stream::new(dictionary! {}, [0u8, 1].repeat(0x10000));
    let _ = map.compress();
    let map = doc.add_object(map);
    let descriptor = doc.add_object(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => "GlyphLessFont",
        "Flags" => 5,
        "FontBBox" => vec![0.into(), 0.into(), GLYPH_WIDTH.into(), 1000.into()],
        "ItalicAngle" => 0,
        "Ascent" => 1000,
        "Descent" => 0,
        "CapHeight" => 1000,
        "StemV" => 80,
        "FontFile2" => file,
    });
    let cid_font = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "GlyphLessFont",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0,
        },
        "FontDescriptor" => descriptor,
        "DW" => GLYPH_WIDTH,
        "CIDToGIDMap" => map,
    });
    let mut to_unicode = Stream::new(dictionary! {}, identity_to_unicode().into_bytes());
    let _ = to_unicode.compress();
    let to_unicode = doc.add_object(to_unicode);
    doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "GlyphLessFont",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![cid_font.into()],
        "ToUnicode" => to_unicode,
    })
}

/// A CMap giving each two-byte code its own value as Unicode (surrogates
/// left out).
fn identity_to_unicode() -> String {
    let ranges: Vec<String> = (0u32..=0xFF)
        .filter(|high| !(0xD8..=0xDF).contains(high))
        .map(|high| format!("<{high:02X}00> <{high:02X}FF> <{high:02X}00>"))
        .collect();
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    for chunk in ranges.chunks(100) {
        cmap.push_str(&format!(
            "{} beginbfrange\n{}\nendbfrange\n",
            chunk.len(),
            chunk.join("\n")
        ));
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    cmap
}

/// A TrueType font with two empty glyphs, each 500 units wide (1000 per em).
fn glyphless_font() -> Vec<u8> {
    let be16 = |values: &[i32]| -> Vec<u8> {
        values
            .iter()
            .flat_map(|&v| (v as u16).to_be_bytes())
            .collect()
    };
    let version = 0x0001_0000u32.to_be_bytes();
    let mut head = Vec::new();
    head.extend(version);
    head.extend(version);
    head.extend(0u32.to_be_bytes());
    head.extend(0x5F0F_3CF5u32.to_be_bytes());
    // Flags, units per em, created and modified dates.
    head.extend(be16(&[0x000B, 1000]));
    head.extend([0; 16]);
    // Box, style, smallest size, direction, short offsets, glyph format.
    head.extend(be16(&[0, 0, GLYPH_WIDTH as i32, 1000, 0, 3, 2, 0, 0]));
    let mut hhea = Vec::new();
    hhea.extend(version);
    hhea.extend(be16(&[
        1000,
        0,
        0,
        GLYPH_WIDTH as i32,
        0,
        0,
        0,
        1,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        2,
    ]));
    let mut maxp = Vec::new();
    maxp.extend(version);
    maxp.extend(be16(&[2, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0]));
    let hmtx = be16(&[GLYPH_WIDTH as i32, 0, GLYPH_WIDTH as i32, 0]);
    let loca = be16(&[0, 0, 0]);
    crate::pdf_fonts::truetype_file(vec![
        (*b"glyf", Vec::new()),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
    ])
    .unwrap_or_default()
}

/// The page draws some text (visible or not).
pub(crate) fn has_text(page: &Page<'_>) -> bool {
    let (width, height) = page.render_dimensions();
    let cache = InterpreterCache::new();
    let mut finder = TextFinder(false);
    let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut context = Context::new(
            page.initial_transform(true).to_kurbo(),
            kurbo::Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
            &cache,
            page.xref(),
            InterpreterSettings::default(),
        );
        interpret_page(page, &mut context, &mut finder);
    }));
    // A page that cannot be read is left alone.
    read.is_err() || finder.0
}

/// A device that notes whether any glyph is drawn.
struct TextFinder(bool);

impl<'a> Device<'a> for TextFinder {
    fn draw_path(&mut self, _: &kurbo::BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, _: DrawProps<'a>, _: &DrawMode) {
        self.0 |= !run.glyphs().is_empty();
    }
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_font_has_two_empty_glyphs() {
        let font = glyphless_font();
        let face = ttf_parser::Face::parse(&font, 0).unwrap();
        assert_eq!(face.number_of_glyphs(), 2);
        assert_eq!(face.units_per_em(), 1000);
        assert_eq!(face.glyph_hor_advance(ttf_parser::GlyphId(1)), Some(500));
        assert!(face.glyph_bounding_box(ttf_parser::GlyphId(1)).is_none());
    }

    #[test]
    fn every_unicode_character_maps_to_itself() {
        let cmap = identity_to_unicode();
        assert_eq!(cmap.matches("beginbfrange").count(), 3);
        assert!(cmap.contains("<0000> <00FF> <0000>"));
        assert!(cmap.contains("<2000> <20FF> <2000>"));
        assert!(!cmap.contains("<D800>"));
    }

    #[test]
    fn lines_follow_the_page_and_are_stretched_to_their_length() {
        // A line 400 × 40 pixels read at 2 pixels per point on a page whose
        // picture was flipped (pixels downwards, points upwards).
        let line = TextLine {
            text: "Città €".into(),
            confidence: 1.0,
            cx: 300.0,
            cy: 100.0,
            ux: 1.0,
            uy: 0.0,
            half_len: 200.0,
            half_h: 20.0,
        };
        let to_page = kurbo::Affine::new([0.5, 0.0, 0.0, -0.5, 0.0, 800.0]);
        let placed = place(&[line], to_page);
        assert_eq!(placed.len(), 1);
        let p = &placed[0];
        assert_eq!(
            (p.origin, p.along, p.up),
            ((50.0, 740.0), (1.0, 0.0), (0.0, 1.0))
        );
        assert_eq!((p.length, p.height), (200.0, 20.0));
        let drawn = drawing(&placed);
        // Seven characters at 20 points, 10 points each: 70 points stretched to 200.
        assert!(drawn.contains("/KonvertOCR 20.00 Tf 285.714 Tz 1.00000 0.00000 0.00000 1.00000 50.000 740.000 Tm <004300690074007400E0002020AC> Tj"), "{drawn}");
        assert!(drawn.starts_with("BT\n3 Tr\n") && drawn.ends_with("ET\n"));
    }
}
