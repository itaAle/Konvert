//! PDF → DOCX: the text and pictures of each page become paragraphs that can
//! be edited in Word.
//!
//! hayro interprets every page with a device that collects the glyphs (their
//! Unicode text, position, size, font and color; also the invisible text of
//! scanned pages that were read by OCR) and the pictures. Glyphs become lines
//! (same baseline), lines are split where a large gap separates columns or
//! table cells, two-column pages are read one column after the other, and
//! lines become paragraphs (spacing, indents, alignment, list markers,
//! headings by size). Drawings, table borders and exact positions are not
//! kept: the result is a document to edit, not a copy of the page.
//!
//! Pages without any text (scans) are read by OCR when an engine is given:
//! each line read becomes glyphs spread along it, in a frame where the text
//! runs straight across (a page scanned askew or sideways is straightened),
//! and goes through the same steps. The same reading gives the plain text of
//! PDFs and of pictures (TXT).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use hayro::hayro_interpret::font::{Glyph as PdfGlyph, GlyphRun};
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageData, ImageDrawProps,
    InterpreterCache, InterpreterSettings, LumaData, Paint, SoftMask, TransformExt, interpret_page,
};
use hayro::kurbo;

use image::GrayImage;

use super::read::{
    Align, Baseline, Block, Document, Inline, LineSpacing, PageSetup, Paragraph, RunProps, Section,
};
use crate::error::CoreError;
use crate::ocr::{Ocr, TextLine};
use crate::pdf;
use crate::pdf_compress::encode_jpeg;
use crate::pdf_ocr;

/// Size of a pixel of a picture read by OCR, in points (as if scanned at
/// 300 dpi).
const PICTURE_POINT: f32 = 72.0 / 300.0;

/// What the conversion could not keep.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    pub pages: u32,
    /// Pages without any text (scans not read by OCR, or where OCR found
    /// nothing): kept as pictures.
    pub pages_without_text: u32,
    /// Pages read by OCR.
    pub pages_read: u32,
}

/// Converts a PDF into a DOCX; pages without text are read by `ocr` when
/// given. `progress` gets the fraction of pages done. Blocking: run it on a
/// worker thread.
pub fn pdf_to_docx(
    data: &[u8],
    ocr: Option<&Ocr>,
    cancel: &AtomicBool,
    mut progress: impl FnMut(f64),
) -> Result<(Vec<u8>, Report), CoreError> {
    let (document, report) = read_pdf(data, ocr, cancel, &mut progress)?;
    let bytes = super::write::to_docx(&document)?;
    progress(1.0);
    Ok((bytes, report))
}

/// The text of a PDF (see [`super::text::plain_text`]); pages without text
/// are read by `ocr` when given. Blocking: run it on a worker thread.
pub fn pdf_to_text(
    data: &[u8],
    ocr: Option<&Ocr>,
    cancel: &AtomicBool,
    mut progress: impl FnMut(f64),
) -> Result<(String, Report), CoreError> {
    let (document, report) = read_pdf(data, ocr, cancel, &mut progress)?;
    progress(1.0);
    Ok((super::text::plain_text(&document), report))
}

/// The text of a picture, read by OCR. Blocking: run it on a worker thread.
pub fn picture_to_text(
    picture: &GrayImage,
    ocr: &Ocr,
    cancel: &AtomicBool,
) -> Result<String, CoreError> {
    let lines = ocr.read(picture, cancel)?;
    let page = ocr_page(
        &lines,
        (picture.width() as f32, picture.height() as f32),
        PICTURE_POINT,
        Vec::new(),
    );
    let (document, _) = build(vec![page]);
    Ok(super::text::plain_text(&document))
}

/// Every page of a PDF as paragraphs (90% of the progress).
fn read_pdf(
    data: &[u8],
    ocr: Option<&Ocr>,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(f64),
) -> Result<(Document, Report), CoreError> {
    let pdf = pdf::open(data.to_vec())?;
    let pages = pdf.pages();
    let cache = InterpreterCache::new();
    let render_cache = hayro::RenderCache::new();
    let mut pages_read = 0;
    let mut collected = Vec::with_capacity(pages.len());
    for (i, page) in pages.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        let (width, height) = page.render_dimensions();
        let mut device = Collector::default();
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut context = Context::new(
                page.initial_transform(true).to_kurbo(),
                kurbo::Rect::new(0.0, 0.0, width as f64, height as f64),
                &cache,
                page.xref(),
                InterpreterSettings::default(),
            );
            interpret_page(page, &mut context, &mut device);
        }));
        if ok.is_err() {
            return Err(CoreError::CorruptInput {
                details: format!("PDF: page {} could not be read", i + 1),
            });
        }
        let page_area = (width * height) as f64;
        let mut content = PageContent {
            width,
            height,
            glyphs: device.glyphs,
            pictures: device.pictures,
            // A colored page background is not a box behind some text.
            fills: device
                .fills
                .into_iter()
                .filter(|(r, _)| r.area() < page_area * 0.5)
                .collect(),
        };
        if let Some(ocr) = ocr
            && content.glyphs.is_empty()
        {
            let scale = pdf_ocr::ocr_scale(width, height);
            let settings = InterpreterSettings::default();
            if let Some(pixmap) = pdf::draw(page, &render_cache, &settings, scale) {
                let lines = ocr.read(&pdf_ocr::gray(&pixmap), cancel)?;
                pages_read += 1;
                if !lines.is_empty() {
                    let pictures = std::mem::take(&mut content.pictures);
                    let size = (width * scale, height * scale);
                    content = ocr_page(&lines, size, 1.0 / scale, pictures);
                }
            }
        }
        collected.push(content);
        progress((i + 1) as f64 / pages.len().max(1) as f64 * 0.9);
    }
    let (document, mut report) = build(collected);
    report.pages_read = pages_read;
    Ok((document, report))
}

/// A page of `size` pixels read by OCR (pixels of `point` points), as
/// glyphs: the characters of each line spread evenly along it. A page whose
/// text runs askew or sideways is turned so that it runs straight across;
/// its `pictures` (placed on the page as it was) are then left out.
fn ocr_page(
    lines: &[TextLine],
    size: (f32, f32),
    point: f32,
    pictures: Vec<PictureBox>,
) -> PageContent {
    // The main direction of the text, weighted by the length of the lines.
    let (dx, dy) = lines.iter().fold((0.0f32, 0.0f32), |a, l| {
        (a.0 + l.ux * l.half_len, a.1 + l.uy * l.half_len)
    });
    let mut angle = dy.atan2(dx);
    if angle.abs() < 0.5f32.to_radians() {
        angle = 0.0;
    }
    let (sin, cos) = (-angle).sin_cos();
    let (mx, my) = (size.0 / 2.0, size.1 / 2.0);
    let turn = |x: f32, y: f32| {
        let (x, y) = (x - mx, y - my);
        (x * cos - y * sin + mx, x * sin + y * cos + my)
    };
    let corners = [(0.0, 0.0), (size.0, 0.0), (0.0, size.1), size].map(|(x, y)| turn(x, y));
    let (x0, y0) = corners
        .iter()
        .fold((f32::MAX, f32::MAX), |a, c| (a.0.min(c.0), a.1.min(c.1)));
    let (x1, y1) = corners
        .iter()
        .fold((f32::MIN, f32::MIN), |a, c| (a.0.max(c.0), a.1.max(c.1)));
    let font = std::rc::Rc::new(FontStyle {
        family: "Arial".into(),
        bold: false,
        italic: false,
    });
    let mut glyphs = Vec::new();
    for line in lines {
        let chars: Vec<char> = line.text.chars().collect();
        let (cx, cy) = turn(line.cx, line.cy);
        let (cx, cy) = ((cx - x0) * point, (cy - y0) * point);
        let (half_len, half_h) = (line.half_len * point, line.half_h * point);
        let width = 2.0 * half_len / chars.len().max(1) as f32;
        for (i, c) in chars.into_iter().enumerate() {
            glyphs.push(GlyphBox {
                x: cx - half_len + i as f32 * width,
                // The box reaches a little above and below the letters.
                y: cy + 0.5 * half_h,
                width,
                size: 1.5 * half_h,
                text: c.to_string(),
                font: font.clone(),
                color: [0, 0, 0],
            });
        }
    }
    PageContent {
        width: (x1 - x0) * point,
        height: (y1 - y0) * point,
        glyphs,
        pictures: if angle == 0.0 { pictures } else { Vec::new() },
        fills: Vec::new(),
    }
}

// --- Collecting --------------------------------------------------------------------

/// A font as the conversion uses it.
#[derive(Debug, Clone, PartialEq)]
struct FontStyle {
    family: String,
    bold: bool,
    italic: bool,
}

#[derive(Debug, Clone)]
struct GlyphBox {
    /// Left edge and baseline, in points from the top left of the page.
    x: f32,
    y: f32,
    /// Advance (width).
    width: f32,
    size: f32,
    text: String,
    font: std::rc::Rc<FontStyle>,
    color: [u8; 3],
}

#[derive(Debug, Clone)]
struct PictureBox {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    /// PNG or JPEG.
    data: std::rc::Rc<Vec<u8>>,
}

struct PageContent {
    width: f32,
    height: f32,
    glyphs: Vec<GlyphBox>,
    pictures: Vec<PictureBox>,
    /// Colored boxes (title bars, table headings), in drawing order.
    fills: Vec<(kurbo::Rect, [u8; 3])>,
}

#[derive(Default)]
struct Collector {
    glyphs: Vec<GlyphBox>,
    pictures: Vec<PictureBox>,
    fills: Vec<(kurbo::Rect, [u8; 3])>,
    fonts: HashMap<u128, std::rc::Rc<FontStyle>>,
    /// Pictures already encoded, by their PDF object (logos repeat on every page).
    encoded: HashMap<(i32, i32), std::rc::Rc<Vec<u8>>>,
}

impl Collector {
    fn font(&mut self, glyph: &PdfGlyph) -> std::rc::Rc<FontStyle> {
        let PdfGlyph::Outline(outline) = glyph else {
            return std::rc::Rc::new(FontStyle {
                family: "Arial".into(),
                bold: false,
                italic: false,
            });
        };
        let key = outline.font_cache_key();
        if let Some(font) = self.fonts.get(&key) {
            return font.clone();
        }
        let data = outline.font_data();
        let name = data
            .as_ref()
            .and_then(|d| d.postscript_name.clone())
            .unwrap_or_default();
        let lower = name.to_ascii_lowercase();
        let bold = data
            .as_ref()
            .and_then(|d| d.weight)
            .is_some_and(|w| w >= 600)
            || ["bold", "black", "heavy", "semibold", "demi"]
                .iter()
                .any(|k| lower.contains(k));
        let italic = data.as_ref().is_some_and(|d| d.is_italic)
            || lower.contains("italic")
            || lower.contains("oblique");
        let serif = data.as_ref().is_some_and(|d| d.is_serif);
        let mono = data.as_ref().is_some_and(|d| d.is_monospace);
        let family = family_name(&name).unwrap_or_else(|| {
            if mono {
                "Courier New".into()
            } else if serif {
                "Times New Roman".into()
            } else {
                "Arial".into()
            }
        });
        let font = std::rc::Rc::new(FontStyle {
            family,
            bold,
            italic,
        });
        self.fonts.insert(key, font.clone());
        font
    }
}

impl<'a> Device<'a> for Collector {
    fn draw_path(&mut self, path: &kurbo::BezPath, props: DrawProps<'a>, mode: &DrawMode) {
        let (DrawMode::Fill(_) | DrawMode::FillAndStroke(..), Paint::Color(color)) =
            (mode, &props.paint)
        else {
            return;
        };
        let [r, g, b, a] = color.to_rgba().to_rgba8();
        let rect = kurbo::Shape::bounding_box(&(props.transform * path.clone()));
        // White boxes and thin rules do not color any text.
        if a < 128 || [r, g, b] == [255, 255, 255] || rect.width() < 24.0 || rect.height() < 6.0 {
            return;
        }
        self.fills.push((rect, [r, g, b]));
    }
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, _: &DrawMode) {
        let color = match &props.paint {
            Paint::Color(c) => {
                let [r, g, b, _] = c.to_rgba().to_rgba8();
                [r, g, b]
            }
            Paint::Pattern(_) => [0, 0, 0],
        };
        for glyph in run.glyphs() {
            let Some(text) = glyph.as_unicode().map(|u| match u {
                BfString::Char(c) => c.to_string(),
                BfString::String(s) => s,
            }) else {
                continue;
            };
            let t = props.transform * glyph.transform();
            let [a, b, c, d, e, f] = t.as_coeffs();
            // Text turned sideways is not part of the reading order.
            if b.abs() > a.abs() * 0.2 || a <= 0.0 {
                continue;
            }
            let size = (c.hypot(d) * 1000.0) as f32;
            if !(1.0..=500.0).contains(&size) {
                continue;
            }
            let advance = match &**glyph {
                PdfGlyph::Outline(o) => o.advance_width(),
                PdfGlyph::Type3(_) => None,
            }
            .map(|w| (w as f64 * a.hypot(b)) as f32)
            .filter(|w| *w > 0.0)
            .unwrap_or(size * 0.5 * text.chars().count() as f32);
            let font = self.font(glyph);
            self.glyphs.push(GlyphBox {
                x: e as f32,
                y: f as f32,
                width: advance,
                size,
                text,
                font,
                color,
            });
        }
    }
    fn draw_image(&mut self, image: Image<'a, '_>, props: ImageDrawProps<'a>) {
        let Image::Raster(raster) = image else {
            return;
        };
        let (w, h) = (raster.width(), raster.height());
        // The picture's corners on the page.
        let corners = [
            (0.0, 0.0),
            (w as f64, 0.0),
            (0.0, h as f64),
            (w as f64, h as f64),
        ]
        .map(|(x, y)| props.transform * kurbo::Point::new(x, y));
        let (x0, x1) = corners.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
            (lo.min(p.x), hi.max(p.x))
        });
        let (y0, y1) = corners.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
            (lo.min(p.y), hi.max(p.y))
        });
        let (width, height) = ((x1 - x0) as f32, (y1 - y0) as f32);
        // Rules and tiny decorations are not worth a picture.
        if width < 12.0 || height < 12.0 {
            return;
        }
        let id = raster.stream().obj_id();
        let key = (id.obj_number, id.gen_number);
        let data = match self.encoded.get(&key).filter(|_| key.0 > 0) {
            Some(data) => data.clone(),
            None => {
                let mut encoded = None;
                raster.with_rgba(|color, alpha| encoded = encode_picture(color, alpha), None);
                let Some(data) = encoded else { return };
                let data = std::rc::Rc::new(data);
                if key.0 > 0 {
                    self.encoded.insert(key, data.clone());
                }
                data
            }
        };
        self.pictures.push(PictureBox {
            x: x0 as f32,
            y: y0 as f32,
            width,
            height,
            data,
        });
    }
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// A decoded picture as PNG (transparency or few colors) or JPEG (photos).
fn encode_picture(color: ImageData, alpha: Option<LumaData>) -> Option<Vec<u8>> {
    let (pixels, channels, width, height) = match color {
        ImageData::Rgb(rgb) => (rgb.data, 3usize, rgb.width, rgb.height),
        ImageData::Luma(luma) => (luma.data, 1usize, luma.width, luma.height),
    };
    let alpha =
        alpha.filter(|a| a.width == width && a.height == height && a.data.iter().any(|v| *v < 255));
    // Drawings and screenshots (few colors) stay sharp as PNG; photos are JPEG.
    let few_colors = {
        let mut seen = std::collections::HashSet::new();
        pixels.chunks_exact(channels).all(|p| {
            let key = p.iter().fold(0u32, |k, v| k << 8 | u32::from(*v));
            seen.insert(key);
            seen.len() <= 256
        })
    };
    match alpha {
        None if !few_colors => encode_jpeg(&pixels, channels, width, height, 85),
        None => Some(super::compress::plain_png(&pixels, channels, width, height)),
        Some(alpha) => {
            let rgba: Vec<u8> = (0..(width * height) as usize)
                .flat_map(|i| {
                    let a = alpha.data[i];
                    if channels == 1 {
                        let v = pixels[i];
                        [v, v, v, a]
                    } else {
                        [pixels[i * 3], pixels[i * 3 + 1], pixels[i * 3 + 2], a]
                    }
                })
                .collect();
            Some(super::compress::plain_png(&rgba, 4, width, height))
        }
    }
}

/// A readable family name from a PostScript name: `ABCDEF+TimesNewRomanPS-BoldMT`
/// → `Times New Roman`.
fn family_name(postscript: &str) -> Option<String> {
    let name = postscript
        .split_once('+')
        .filter(|(prefix, _)| prefix.len() == 6)
        .map_or(postscript, |(_, rest)| rest);
    let base = name.split(['-', ',']).next()?.trim();
    let base = base
        .trim_end_matches("PSMT")
        .trim_end_matches("MT")
        .trim_end_matches("PS");
    if base.is_empty() {
        return None;
    }
    // Known names whose spelling differs from the family.
    let known = [
        ("TimesNewRoman", "Times New Roman"),
        ("CourierNew", "Courier New"),
        ("Helvetica", "Arial"),
        ("ArialNarrow", "Arial Narrow"),
        ("SegoeUI", "Segoe UI"),
        ("Times", "Times New Roman"),
    ];
    if let Some((_, family)) = known.iter().find(|(ps, _)| *ps == base) {
        return Some((*family).to_owned());
    }
    // Words start at a capital that follows a lowercase letter.
    let mut out = String::new();
    let mut previous_lower = false;
    for c in base.chars() {
        if c.is_ascii_uppercase() && previous_lower {
            out.push(' ');
        }
        previous_lower = c.is_ascii_lowercase();
        out.push(c);
    }
    Some(out)
}

// --- Layout --------------------------------------------------------------------

/// A line of text (or a piece of it, between large gaps).
#[derive(Debug, Clone)]
struct Line {
    glyphs: Vec<GlyphBox>,
    x0: f32,
    x1: f32,
    baseline: f32,
    size: f32,
    /// Pieces of the same printed line share this number.
    row: usize,
}

impl Line {
    fn new(glyphs: Vec<GlyphBox>, row: usize) -> Self {
        let x0 = glyphs.iter().map(|g| g.x).fold(f32::MAX, f32::min);
        let x1 = glyphs
            .iter()
            .map(|g| g.x + g.width)
            .fold(f32::MIN, f32::max);
        // The baseline and size of most of the line's characters.
        let main = most_common(glyphs.iter().map(|g| ((g.size * 2.0).round() as i32, g.y)));
        let size = main.0 as f32 / 2.0;
        Self {
            baseline: main.1,
            size,
            x0,
            x1,
            glyphs,
            row,
        }
    }

    /// The text, with spaces where the glyphs leave a gap.
    fn text(&self) -> String {
        runs(self)
            .iter()
            .map(|r| match r {
                Inline::Text(t, _) => t.as_str(),
                _ => "",
            })
            .collect()
    }
}

/// The (size, baseline) shared by the most glyphs.
fn most_common(items: impl Iterator<Item = (i32, f32)>) -> (i32, f32) {
    let mut counts: HashMap<i32, (usize, f32)> = HashMap::new();
    for (size, y) in items {
        let entry = counts.entry(size).or_insert((0, y));
        entry.0 += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(size, (n, _))| (*n, *size))
        .map(|(size, (_, y))| (size, y))
        .unwrap_or((22, 0.0))
}

/// Glyphs grouped into lines by baseline, each split where a gap is wider
/// than three spaces (columns, table cells): the pieces of one printed line
/// share a row number. Superscripts and subscripts (smaller, a little higher
/// or lower) join the line they belong to.
fn lines(mut glyphs: Vec<GlyphBox>) -> Vec<Line> {
    glyphs.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    // Glyphs on the same baseline.
    let mut rows: Vec<Vec<GlyphBox>> = Vec::new();
    for glyph in glyphs {
        match rows.last_mut() {
            Some(row) if (glyph.y - row[0].y).abs() < row[0].size.min(glyph.size) * 0.15 => {
                row.push(glyph)
            }
            _ => rows.push(vec![glyph]),
        }
    }
    let shape: Vec<(f32, f32, f32, f32)> = rows
        .iter()
        .map(|row| {
            let line = Line::new(row.clone(), 0);
            (line.baseline, line.size, line.x0, line.x1)
        })
        .collect();
    // A row in a smaller size, within the height of a row in a larger one
    // that it overlaps sideways, belongs to that row.
    let mut owner: Vec<usize> = (0..rows.len()).collect();
    for i in 0..rows.len() {
        let (y, size, x0, x1) = shape[i];
        let best = (0..rows.len())
            .filter(|&j| j != i)
            .filter(|&j| {
                let (yj, sj, x0j, x1j) = shape[j];
                size <= sj * 0.9
                    && y >= yj - sj * 0.75
                    && y <= yj + sj * 0.6
                    && x0 <= x1j + sj
                    && x1 >= x0j - sj
            })
            .min_by(|&a, &b| (shape[a].0 - y).abs().total_cmp(&(shape[b].0 - y).abs()));
        if let Some(j) = best {
            owner[i] = j;
        }
    }
    // Follow chains (a small row attached to another small row).
    for i in 0..rows.len() {
        let mut j = owner[i];
        for _ in 0..4 {
            if owner[j] == j {
                break;
            }
            j = owner[j];
        }
        owner[i] = j;
    }
    let mut merged: Vec<Vec<GlyphBox>> = vec![Vec::new(); rows.len()];
    for (i, row) in rows.into_iter().enumerate() {
        merged[owner[i]].extend(row);
    }
    let mut out = Vec::new();
    for (row_number, mut row) in merged.into_iter().enumerate() {
        if row.is_empty() {
            continue;
        }
        row.sort_by(|a, b| a.x.total_cmp(&b.x));
        let mut piece: Vec<GlyphBox> = Vec::new();
        for glyph in row {
            if let Some(last) = piece.last() {
                let gap = glyph.x - (last.x + last.width);
                if gap > (last.size.max(glyph.size) * 1.6).max(12.0) {
                    out.push(Line::new(std::mem::take(&mut piece), row_number));
                }
            }
            piece.push(glyph);
        }
        if !piece.is_empty() {
            out.push(Line::new(piece, row_number));
        }
    }
    out.sort_by(|a, b| {
        a.baseline
            .total_cmp(&b.baseline)
            .then(a.x0.total_cmp(&b.x0))
    });
    out
}

/// Text of a line with spaces where the glyphs leave a gap, as runs of the
/// same formatting; superscripts and subscripts by their baseline.
fn runs(line: &Line) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    let mut previous: Option<&GlyphBox> = None;
    for glyph in &line.glyphs {
        let mut text = String::new();
        if let Some(p) = previous {
            let gap = glyph.x - (p.x + p.width);
            if gap > p.size.min(glyph.size) * 0.18
                && !p.text.ends_with(' ')
                && !glyph.text.starts_with(' ')
            {
                text.push(' ');
            }
        }
        text.push_str(&glyph.text);
        previous = Some(glyph);
        let shift = line.baseline - glyph.y;
        let small = glyph.size < line.size * 0.85;
        let baseline = if small && shift > line.size * 0.2 {
            Baseline::Super
        } else if small && shift < -line.size * 0.1 {
            Baseline::Sub
        } else {
            Baseline::Normal
        };
        let props = RunProps {
            font: glyph.font.family.clone(),
            size: round_half(if baseline == Baseline::Normal {
                glyph.size
            } else {
                line.size
            }),
            bold: glyph.font.bold,
            italic: glyph.font.italic,
            color: Some(glyph.color).filter(|c| *c != [0, 0, 0]),
            baseline,
            ..RunProps::default()
        };
        match out.last_mut() {
            Some(Inline::Text(last, last_props)) if *last_props == props => last.push_str(&text),
            _ => out.push(Inline::Text(text, props)),
        }
    }
    out
}

fn round_half(size: f32) -> f32 {
    (size * 2.0).round() / 2.0
}

/// Bullets and numbers that start list items.
fn list_marker(text: &str) -> bool {
    let t = text.trim_start();
    let first = t.chars().next().unwrap_or(' ');
    if matches!(
        first,
        '•' | '◦' | '▪' | '■' | '–' | '-' | '●' | '○' | '►' | '➢' | '✓' | '*'
    ) {
        return t.chars().nth(1).is_some_and(char::is_whitespace);
    }
    let marker: String = t.chars().take_while(|c| !c.is_whitespace()).collect();
    let stripped = marker.trim_end_matches(['.', ')']).trim_start_matches('(');
    marker.len() <= 5
        && marker.len() > stripped.len()
        && !stripped.is_empty()
        && (stripped.chars().all(|c| c.is_ascii_digit())
            || (stripped.len() == 1 && stripped.chars().all(|c| c.is_ascii_alphabetic())))
}

/// One block of reading order: a stream of lines (a column), or a picture.
enum Item {
    Lines(Vec<Line>),
    Picture(PictureBox),
}

/// Two columns when most lines are split at the same place.
fn column_split(lines: &[Line], page_width: f32) -> Option<f32> {
    let middle = page_width / 2.0;
    let left: Vec<&Line> = lines.iter().filter(|l| l.x1 < middle + 20.0).collect();
    let right: Vec<&Line> = lines.iter().filter(|l| l.x0 > middle - 20.0).collect();
    let across = lines
        .iter()
        .filter(|l| l.x0 < middle - 20.0 && l.x1 > middle + 20.0)
        .count();
    let enough = |n: usize| n >= 6 && n * 4 >= lines.len();
    if !enough(left.len()) || !enough(right.len()) || across * 4 > lines.len() {
        return None;
    }
    let gutter = (left.iter().map(|l| l.x1).fold(f32::MIN, f32::max)
        + right.iter().map(|l| l.x0).fold(f32::MAX, f32::min))
        / 2.0;
    Some(gutter)
}

fn build(pages: Vec<PageContent>) -> (Document, Report) {
    let mut report = Report {
        pages: pages.len() as u32,
        ..Report::default()
    };
    // The size of body text: the most common one in the whole document.
    let mut sizes: HashMap<i32, usize> = HashMap::new();
    for page in &pages {
        for g in &page.glyphs {
            *sizes.entry((g.size * 2.0).round() as i32).or_default() += g.text.chars().count();
        }
    }
    let body_size = sizes
        .iter()
        .max_by_key(|(size, n)| (**n, -**size))
        .map_or(11.0, |(size, _)| *size as f32 / 2.0);
    // Heading levels: the larger sizes, largest first.
    let mut heading_sizes: Vec<i32> = sizes
        .keys()
        .copied()
        .filter(|s| *s as f32 / 2.0 >= body_size * 1.12)
        .collect();
    heading_sizes.sort_unstable_by(|a, b| b.cmp(a));
    heading_sizes.truncate(3);

    let mut document = Document::default();
    let mut setup = PageSetup::default();
    let mut picture_count = 0;
    for (index, page) in pages.into_iter().enumerate() {
        if index == 0 {
            setup = page_setup(&page);
        }
        let text_left = setup.left;
        let text_width = (page.width - setup.left - setup.right).max(72.0);
        let has_text = !page.glyphs.is_empty();
        let mut pictures = page.pictures;
        if !has_text {
            report.pages_without_text += 1;
        } else {
            // A scan behind OCR text: the text is what matters.
            pictures.retain(|p| p.width * p.height < page.width * page.height * 0.8);
        }
        let all_lines = lines(page.glyphs);
        let items = reading_order(all_lines, pictures, page.width);
        let mut first_on_page = true;
        let mut previous_bottom: Option<f32> = None;
        for item in items {
            let mut blocks = match item {
                Item::Picture(picture) => {
                    picture_count += 1;
                    let part = format!(
                        "picture{picture_count}.{}",
                        if picture.data.starts_with(&[0xFF, 0xD8]) {
                            "jpeg"
                        } else {
                            "png"
                        }
                    );
                    document.media.insert(part.clone(), (*picture.data).clone());
                    // As wide as on the page, never wider than the text.
                    let scale = (text_width / picture.width).min(1.0);
                    let centered =
                        ((picture.x + picture.width / 2.0) - page.width / 2.0).abs() < 24.0;
                    let before = previous_bottom.map_or(0.0, |b| (picture.y - b).clamp(0.0, 36.0));
                    previous_bottom = Some(picture.y + picture.height);
                    vec![Paragraph {
                        align: if centered { Align::Center } else { Align::Left },
                        before,
                        after: 0.0,
                        line: LineSpacing::Multiple(1.0),
                        left: if centered {
                            0.0
                        } else {
                            (picture.x - text_left).max(0.0)
                        },
                        right: 0.0,
                        first_line: 0.0,
                        outline: None,
                        page_break_before: false,
                        keep_lines: false,
                        keep_next: false,
                        shading: None,
                        mark: RunProps::default(),
                        label: None,
                        tabs: Vec::new(),
                        inlines: vec![Inline::Picture {
                            part,
                            width: picture.width * scale,
                            height: picture.height * scale,
                        }],
                    }]
                }
                Item::Lines(stream) => {
                    let bottom = stream.last().map(|l| l.baseline);
                    let mut blocks =
                        paragraphs(&stream, text_left, &heading_sizes, previous_bottom);
                    shade(&mut blocks, &stream, &page.fills);
                    previous_bottom = bottom.or(previous_bottom);
                    blocks
                }
            };
            if first_on_page
                && index > 0
                && let Some(first) = blocks.first_mut()
            {
                first.page_break_before = true;
                first.before = 0.0;
            }
            first_on_page &= blocks.is_empty();
            document
                .body
                .extend(blocks.into_iter().map(Block::Paragraph));
        }
    }
    document.sections = vec![Section {
        end: document.body.len(),
        page: setup,
        columns: 1,
        new_page: true,
        ..Section::default()
    }];
    (document, report)
}

/// Paragraphs printed on a colored box get its color as their shading;
/// light text left on white (its box was not a plain rectangle) turns dark,
/// so it stays readable.
fn shade(paragraphs: &mut [Paragraph], stream: &[Line], fills: &[(kurbo::Rect, [u8; 3])]) {
    let light = |c: [u8; 3]| c.iter().map(|v| *v as u32).sum::<u32>() > 3 * 215;
    for paragraph in paragraphs.iter_mut() {
        // The first line of the paragraph, found by its text.
        let first_text: String = paragraph
            .inlines
            .iter()
            .find_map(|i| match i {
                Inline::Text(t, _) => Some(t.trim().chars().take(12).collect()),
                _ => None,
            })
            .unwrap_or_default();
        let line = stream
            .iter()
            .find(|l| !first_text.is_empty() && l.text().trim_start().starts_with(&first_text));
        if let Some(line) = line {
            let point = kurbo::Point::new(
                (line.x0 + 2.0) as f64,
                (line.baseline - line.size * 0.3) as f64,
            );
            // The last box drawn under the text is the one that shows.
            paragraph.shading = fills
                .iter()
                .rev()
                .find(|(r, _)| r.contains(point))
                .map(|(_, c)| *c);
        }
        if paragraph.shading.is_none_or(light) {
            for inline in &mut paragraph.inlines {
                if let Inline::Text(_, props) | Inline::Tab(props) = inline
                    && props.color.is_some_and(light)
                {
                    props.color = None;
                }
            }
        }
    }
}

/// Margins from where the text of the first page is, within reason.
fn page_setup(page: &PageContent) -> PageSetup {
    let mut setup = PageSetup {
        width: page.width,
        height: page.height,
        ..PageSetup::default()
    };
    if page.glyphs.is_empty() {
        return setup;
    }
    let left = page.glyphs.iter().map(|g| g.x).fold(f32::MAX, f32::min);
    let right = page
        .glyphs
        .iter()
        .map(|g| g.x + g.width)
        .fold(f32::MIN, f32::max);
    let top = page
        .glyphs
        .iter()
        .map(|g| g.y - g.size)
        .fold(f32::MAX, f32::min);
    setup.left = left.clamp(18.0, 108.0);
    setup.right = (page.width - right).clamp(18.0, 108.0);
    setup.top = top.clamp(18.0, 108.0);
    setup.bottom = setup.bottom.min(setup.top.max(36.0));
    setup
}

/// Lines and pictures in reading order: top to bottom, and on two-column
/// pages the left column before the right one (full-width lines, such as
/// titles, separate the column bands).
fn reading_order(lines: Vec<Line>, pictures: Vec<PictureBox>, page_width: f32) -> Vec<Item> {
    // Streams of lines in reading order: the page, or bands of two columns
    // (left, then right) between full-width lines.
    let mut streams: Vec<Vec<Line>> = Vec::new();
    match column_split(&lines, page_width) {
        None => streams.push(lines),
        Some(gutter) => {
            let (mut left, mut right): (Vec<Line>, Vec<Line>) = (Vec::new(), Vec::new());
            for line in lines {
                if line.x0 < gutter - 10.0 && line.x1 > gutter + 10.0 {
                    streams.push(std::mem::take(&mut left));
                    streams.push(std::mem::take(&mut right));
                    streams.push(vec![line]);
                } else if line.x1 <= gutter + 10.0 {
                    left.push(line);
                } else {
                    right.push(line);
                }
            }
            streams.push(left);
            streams.push(right);
        }
    }
    // Pictures go before the first line printed below them, splitting a
    // stream if needed.
    let mut pictures = pictures;
    pictures.sort_by(|a, b| a.y.total_cmp(&b.y));
    let mut pictures = pictures.into_iter().peekable();
    let mut out: Vec<Item> = Vec::new();
    for stream in streams {
        let mut current: Vec<Line> = Vec::new();
        for line in stream {
            while let Some(picture) = pictures.next_if(|p| p.y + p.height <= line.baseline) {
                if !current.is_empty() {
                    out.push(Item::Lines(std::mem::take(&mut current)));
                }
                out.push(Item::Picture(picture));
            }
            current.push(line);
        }
        if !current.is_empty() {
            out.push(Item::Lines(current));
        }
    }
    out.extend(pictures.map(Item::Picture));
    out
}

/// A row printed in pieces: one paragraph, a tab before each piece after
/// the first, with tab stops where the pieces start.
fn tabbed_row(pieces: &[&Line], text_left: f32, previous_bottom: Option<f32>) -> Paragraph {
    let first = pieces[0];
    let left = (first.x0 - text_left).max(0.0);
    let mut inlines = Vec::new();
    let mut tabs = Vec::new();
    for (i, piece) in pieces.iter().enumerate() {
        let runs = runs(piece);
        if i > 0 {
            let props = match runs.first() {
                Some(Inline::Text(_, props)) => props.clone(),
                _ => RunProps::default(),
            };
            inlines.push(Inline::Tab(props));
            tabs.push((piece.x0 - text_left).max(left + 1.0));
        }
        inlines.extend(runs);
    }
    let mark = match inlines.first() {
        Some(Inline::Text(_, props)) => props.clone(),
        _ => RunProps::default(),
    };
    Paragraph {
        align: Align::Left,
        before: previous_bottom.map_or(0.0, |b| {
            (first.baseline - b - first.size * 1.2).clamp(0.0, 24.0)
        }),
        after: 0.0,
        line: LineSpacing::Multiple(1.0),
        left,
        right: 0.0,
        first_line: 0.0,
        outline: None,
        page_break_before: false,
        keep_lines: false,
        keep_next: false,
        shading: None,
        mark,
        label: None,
        tabs,
        inlines,
    }
}

/// Lines of one stream as paragraphs.
fn paragraphs(
    stream: &[Line],
    text_left: f32,
    heading_sizes: &[i32],
    previous_bottom: Option<f32>,
) -> Vec<Paragraph> {
    if stream.is_empty() {
        return Vec::new();
    }
    // Usual distance between baselines in this stream, for its body text.
    let mut pitches: Vec<f32> = stream
        .windows(2)
        .filter(|w| (w[0].size - w[1].size).abs() < 0.5)
        .map(|w| w[1].baseline - w[0].baseline)
        .filter(|p| *p > 0.0)
        .collect();
    pitches.sort_by(f32::total_cmp);
    let pitch_of = |size: f32| -> f32 {
        pitches
            .get(pitches.len() / 2)
            .copied()
            .filter(|p| *p < size * 2.2 && *p > size * 0.9)
            .unwrap_or(size * 1.2)
    };
    let left_edge = stream.iter().map(|l| l.x0).fold(f32::MAX, f32::min);
    let right_edge = stream.iter().map(|l| l.x1).fold(f32::MIN, f32::max);
    let width = (right_edge - left_edge).max(1.0);

    // Rows printed in several pieces (table cells, a label and its value).
    let pieces_of = |row: usize| stream.iter().filter(|l| l.row == row).count();
    // Groups of lines.
    let mut groups: Vec<Vec<&Line>> = Vec::new();
    for (i, line) in stream.iter().enumerate() {
        let previous = i.checked_sub(1).map(|p| &stream[p]);
        if previous.is_some_and(|p| p.row == line.row) {
            if let Some(group) = groups.last_mut() {
                group.push(line);
            }
            continue;
        }
        let tabbed = pieces_of(line.row) > 1 || previous.is_some_and(|p| pieces_of(p.row) > 1);
        let new = tabbed
            || match (previous, groups.last()) {
                (Some(previous), Some(group)) => {
                    let gap = line.baseline - previous.baseline;
                    let pitch = pitch_of(line.size);
                    let size_changed =
                        (line.size - previous.size).abs() > line.size.max(previous.size) * 0.08;
                    let style = |l: &Line| l.glyphs.first().map(|g| (g.font.bold, g.color));
                    let style_changed = style(line) != style(previous);
                    let previous_short = previous.x1 < right_edge - width * 0.15;
                    let indented = line.x0 > left_edge + line.size * 1.5
                        && group.len() > 1
                        && (group[1].x0 - line.x0).abs() > line.size;
                    gap > pitch * 1.45
                        || gap < 0.0
                        || size_changed
                        || list_marker(&line.text())
                        || (previous_short && line.x0 > previous.x0 + line.size)
                        || (previous_short && style_changed)
                        || indented
                }
                _ => true,
            };
        if new {
            groups.push(vec![line]);
        } else if let Some(group) = groups.last_mut() {
            group.push(line);
        }
    }

    let mut out = Vec::new();
    let mut previous_bottom = previous_bottom;
    for group in groups {
        let first = group[0];
        if group.len() > 1 && group.iter().all(|l| l.row == first.row) {
            out.push(tabbed_row(&group, text_left, previous_bottom));
            previous_bottom = Some(first.baseline);
            continue;
        }
        let size = first.size;
        let pitch = if group.len() > 1 {
            (group.last().expect("not empty").baseline - first.baseline) / (group.len() - 1) as f32
        } else {
            pitch_of(size)
        };
        let multiple = ((pitch / (size * 1.2)) * 20.0).round() / 20.0;
        let before = previous_bottom.map_or(0.0, |b| {
            (first.baseline - size - b - (pitch - size)).clamp(0.0, 48.0)
        });
        previous_bottom = Some(group.last().expect("not empty").baseline);

        let rest_left = group.iter().skip(1).map(|l| l.x0).fold(f32::MAX, f32::min);
        let left_x = if group.len() > 1 { rest_left } else { first.x0 };
        let centers_match = group.iter().all(|l| {
            let center = (l.x0 + l.x1) / 2.0;
            (center - (left_edge + right_edge) / 2.0).abs() < width * 0.04
        });
        let align = if centers_match && first.x0 > left_edge + size * 2.0 {
            Align::Center
        } else if group.len() > 2
            && group[..group.len() - 1]
                .iter()
                .all(|l| (l.x1 - right_edge).abs() < size * 1.2)
        {
            Align::Justify
        } else if group.iter().all(|l| (l.x1 - right_edge).abs() < size * 0.8)
            && first.x0 > left_edge + width * 0.3
        {
            Align::Right
        } else {
            Align::Left
        };
        let (left, first_line) = if align == Align::Center || align == Align::Right {
            (0.0, 0.0)
        } else {
            ((left_x - text_left).max(0.0), first.x0 - left_x)
        };

        let mut inlines: Vec<Inline> = Vec::new();
        for (n, line) in group.iter().enumerate() {
            let mut line_runs = runs(line);
            if n > 0 {
                // Lines of a paragraph are joined: a word cut by a hyphen is
                // put back together, other lines are separated by a space.
                let ends_with_hyphen = matches!(inlines.last(), Some(Inline::Text(t, _)) if t.ends_with('-') && t.len() > 1);
                let starts_lower = line.text().chars().next().is_some_and(char::is_lowercase);
                if ends_with_hyphen && starts_lower {
                    if let Some(Inline::Text(t, _)) = inlines.last_mut() {
                        t.pop();
                    }
                } else if let Some(Inline::Text(t, _)) = line_runs.first_mut() {
                    t.insert(0, ' ');
                }
            }
            for run in line_runs.drain(..) {
                match (inlines.last_mut(), run) {
                    (Some(Inline::Text(last, last_props)), Inline::Text(text, props))
                        if *last_props == props =>
                    {
                        last.push_str(&text)
                    }
                    (_, run) => inlines.push(run),
                }
            }
        }
        let size_key = (size * 2.0).round() as i32;
        let outline = heading_sizes
            .iter()
            .position(|s| *s == size_key)
            .map(|level| level as u8)
            .filter(|_| group.len() <= 3);
        let mark = match inlines.first() {
            Some(Inline::Text(_, props)) => props.clone(),
            _ => RunProps::default(),
        };
        out.push(Paragraph {
            align,
            before,
            after: 0.0,
            line: LineSpacing::Multiple(multiple.clamp(1.0, 3.0)),
            left,
            right: 0.0,
            first_line,
            outline,
            page_break_before: false,
            keep_lines: false,
            keep_next: outline.is_some(),
            shading: None,
            mark,
            label: None,
            tabs: Vec::new(),
            inlines,
        });
    }
    out
}

/// The glyphs of each page: text, left edge and baseline (points from the
/// top left of the page as shown) and size.
#[cfg(test)]
pub(crate) fn glyphs_of(data: &[u8]) -> Vec<Vec<(String, f32, f32, f32)>> {
    let pdf = pdf::open(data.to_vec()).unwrap();
    let cache = InterpreterCache::new();
    pdf.pages()
        .iter()
        .map(|page| {
            let (width, height) = page.render_dimensions();
            let mut device = Collector::default();
            let mut context = Context::new(
                page.initial_transform(true).to_kurbo(),
                kurbo::Rect::new(0.0, 0.0, width as f64, height as f64),
                &cache,
                page.xref(),
                InterpreterSettings::default(),
            );
            interpret_page(page, &mut context, &mut device);
            device
                .glyphs
                .into_iter()
                .map(|g| (g.text, g.x, g.y, g.size))
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(x: f32, y: f32, text: &str, size: f32) -> GlyphBox {
        GlyphBox {
            x,
            y,
            width: size * 0.5 * text.chars().count() as f32,
            size,
            text: text.into(),
            font: std::rc::Rc::new(FontStyle {
                family: "Arial".into(),
                bold: false,
                italic: false,
            }),
            color: [0, 0, 0],
        }
    }

    fn word(x: f32, y: f32, text: &str, size: f32) -> Vec<GlyphBox> {
        text.chars()
            .enumerate()
            .map(|(i, c)| glyph(x + i as f32 * size * 0.5, y, &c.to_string(), size))
            .collect()
    }

    #[test]
    fn postscript_names_become_families() {
        assert_eq!(
            family_name("ABCDEF+TimesNewRomanPS-BoldMT").unwrap(),
            "Times New Roman"
        );
        assert_eq!(family_name("Calibri-Bold").unwrap(), "Calibri");
        assert_eq!(family_name("ArialMT").unwrap(), "Arial");
        assert_eq!(
            family_name("SourceSansPro-Regular").unwrap(),
            "Source Sans Pro"
        );
        assert_eq!(family_name("Helvetica").unwrap(), "Arial");
    }

    #[test]
    fn list_markers_are_recognized() {
        assert!(list_marker("• Primo punto"));
        assert!(list_marker("1. Introduzione"));
        assert!(list_marker("a) caso"));
        assert!(!list_marker("(iv) x"));
        assert!(!list_marker("2026 è l'anno"));
        assert!(!list_marker("-5 gradi"));
    }

    #[test]
    fn glyphs_become_lines_words_and_paragraphs() {
        let mut glyphs = Vec::new();
        glyphs.extend(word(72.0, 100.0, "Titolo", 20.0));
        glyphs.extend(word(72.0, 140.0, "uno", 10.0));
        glyphs.extend(word(95.0, 140.0, "due", 10.0));
        glyphs.extend(word(72.0, 152.0, "tre", 10.0));
        glyphs.extend(word(72.0, 190.0, "altro", 10.0));
        let lines = lines(glyphs);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[1].text(), "uno due");
        let paragraphs = paragraphs(&lines, 72.0, &[40], None);
        assert_eq!(paragraphs.len(), 3, "{paragraphs:#?}");
        assert_eq!(paragraphs[0].outline, Some(0));
        let text = |p: &Paragraph| -> String {
            p.inlines
                .iter()
                .map(|i| match i {
                    Inline::Text(t, _) => t.as_str(),
                    _ => "",
                })
                .collect()
        };
        assert_eq!(text(&paragraphs[1]), "uno due tre");
        assert_eq!(text(&paragraphs[2]), "altro");
    }

    #[test]
    fn two_columns_are_read_one_after_the_other() {
        let mut lines_left = Vec::new();
        let mut lines_right = Vec::new();
        for i in 0..8 {
            let y = 100.0 + i as f32 * 14.0;
            lines_left.extend(word(60.0, y, "sinistra", 10.0));
            lines_right.extend(word(330.0, y, "destra", 10.0));
        }
        let mut all = lines_left;
        all.extend(lines_right);
        let lines = lines(all);
        assert!(column_split(&lines, 595.0).is_some());
        let items = reading_order(lines, Vec::new(), 595.0);
        let texts: Vec<String> = items
            .iter()
            .filter_map(|i| match i {
                Item::Lines(l) => Some(l[0].text()),
                Item::Picture(_) => None,
            })
            .collect();
        assert_eq!(texts, ["sinistra", "destra"]);
    }
}
