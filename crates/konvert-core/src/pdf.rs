//! PDF documents, handled without FFmpeg:
//!
//! * analysis ([`probe`]) and rendering of pages to pictures and thumbnails
//!   with hayro (pure Rust, no unsafe code);
//! * pictures laid out on pages ([`place`]), documents assembled or merged
//!   ([`assemble`]) and pages rearranged, turned or split into several
//!   documents ([`edit`]) with lopdf.
//!
//! Konvert never asks for passwords: a PDF that needs one gives
//! [`CoreError::PdfEncrypted`]. PDFs protected only against editing (empty
//! user password) open normally. The PDFs Konvert writes carry no path, user
//! name or date: their only metadata is `Producer: Konvert`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::object::{Array, Dict};
use hayro::hayro_syntax::page::Page;
use hayro::hayro_syntax::{LoadPdfError, Pdf};
use hayro::vello_cpu::Pixmap;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::CoreError;
use crate::metadata::base64;
use crate::probe::{MediaInfo, MediaKind};
use crate::request::{PageMargin, PageOrientation, PageSize, PdfOptions, PdfPage, PdfSplit};

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
    /// Bookmarks or a fillable form, which the PDFs Konvert writes do not keep.
    #[serde(default)]
    pub extras: bool,
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
    let extras = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| has_extras(&pdf)))
        .unwrap_or(false);
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
            extras,
        }),
    })
}

/// The document has bookmarks or form fields.
fn has_extras(pdf: &Pdf) -> bool {
    let xref = pdf.xref();
    let Some(catalog) = xref.get::<Dict>(xref.root_id()) else {
        return false;
    };
    let outline = catalog
        .get::<Dict>(b"Outlines")
        .is_some_and(|o| o.contains_key(b"First"));
    let form = catalog
        .get::<Dict>(b"AcroForm")
        .and_then(|f| f.get::<Array>(b"Fields"))
        .is_some_and(|f| f.raw_iter().next().is_some());
    outline || form
}

/// Pages of a PDF made from another one: `None` means every page as it is.
/// Each page appears at most once; turns are multiples of 90 degrees.
pub fn check_layout(layout: Option<&[PdfPage]>, count: u32) -> Result<Vec<PdfPage>, CoreError> {
    let Some(layout) = layout else {
        return Ok((1..=count)
            .map(|number| PdfPage {
                number,
                rotation: 0,
            })
            .collect());
    };
    let invalid = |details: String| CoreError::InvalidPages { details };
    if layout.is_empty() {
        return Err(invalid("no page left in the document".into()));
    }
    let mut seen = vec![false; count as usize + 1];
    layout
        .iter()
        .map(|page| {
            if page.number == 0 || page.number > count {
                return Err(invalid(format!(
                    "page {} does not exist (the document has {count} pages)",
                    page.number
                )));
            }
            if std::mem::replace(&mut seen[page.number as usize], true) {
                return Err(invalid(format!("page {} is listed twice", page.number)));
            }
            if page.rotation % 90 != 0 {
                return Err(invalid(format!("invalid turn {}°", page.rotation)));
            }
            Ok(PdfPage {
                number: page.number,
                rotation: page.rotation % 360,
            })
        })
        .collect()
}

/// Pages in each document when `pages` pages are split (empty: one document).
/// Cut positions outside the document are ignored.
pub fn split_sizes(
    split: PdfSplit,
    every: u32,
    after: &[u32],
    pages: u32,
) -> Result<Vec<u32>, CoreError> {
    let sizes = match split {
        PdfSplit::None => Vec::new(),
        PdfSplit::Every if every == 0 => {
            return Err(CoreError::InvalidPages {
                details: "a document needs at least one page".into(),
            });
        }
        PdfSplit::Every => (0..pages)
            .step_by(every as usize)
            .map(|start| every.min(pages - start))
            .collect(),
        PdfSplit::After => {
            let mut cuts: Vec<u32> = after
                .iter()
                .copied()
                .filter(|c| (1..pages).contains(c))
                .collect();
            cuts.sort_unstable();
            cuts.dedup();
            cuts.push(pages);
            let mut start = 0;
            cuts.into_iter()
                .map(|cut| cut - std::mem::replace(&mut start, cut))
                .collect()
        }
    };
    Ok(if sizes.len() > 1 { sizes } else { Vec::new() })
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
        let png = draw(page, &cache, &settings, scale)
            .ok_or_else(|| damaged(&format!("page {number} could not be drawn")))?
            .into_png()
            .map_err(|e| CoreError::Io(std::io::Error::other(e.to_string())))?;
        if !on_page(RenderedPage { number, png }) {
            return Err(CoreError::Cancelled);
        }
    }
    Ok(())
}

/// Draws a page on white at `scale` pixels per point. A renderer bug on one
/// strange page must not take the app down: it gives `None`.
fn draw<'a>(
    page: &'a Page<'a>,
    cache: &RenderCache<'a>,
    settings: &InterpreterSettings,
    scale: f32,
) -> Option<Pixmap> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        hayro::render(
            page,
            cache,
            settings,
            &RenderSettings::default(),
            &PixmapSettings {
                x_scale: scale,
                y_scale: scale,
                bg_color: WHITE,
            },
        )
    }))
    .ok()
}

/// Longest side of a page thumbnail, in pixels.
pub const MAX_THUMBNAIL: u32 = 512;
/// Thumbnails drawn by one call of [`thumbnails`].
pub const THUMBNAILS_PER_CALL: usize = 48;

/// A small picture of a page, shown while choosing pages.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Thumbnail {
    /// 1-based page number.
    pub number: u32,
    /// Size of the page in points, turned as the document shows it.
    pub width_pt: f32,
    pub height_pt: f32,
    /// The page as a PNG `data:` URL (empty if the page could not be drawn).
    pub data_url: String,
}

/// Thumbnails of the given pages (at most [`THUMBNAILS_PER_CALL`]), whose
/// longest side is `size` pixels. Pages that do not exist are skipped.
/// Blocking: run it on a worker thread.
pub fn thumbnails(data: Vec<u8>, pages: &[u32], size: u32) -> Result<Vec<Thumbnail>, CoreError> {
    let pdf =
        std::panic::catch_unwind(|| open(data)).map_err(|_| damaged("the reader failed"))??;
    let all = pdf.pages();
    let cache = RenderCache::new();
    let settings = InterpreterSettings::default();
    let size = size.clamp(16, MAX_THUMBNAIL) as f32;
    Ok(pages
        .iter()
        .take(THUMBNAILS_PER_CALL)
        .filter_map(|&number| Some((number, all.get((number as usize).checked_sub(1)?)?)))
        .map(|(number, page)| {
            let (width_pt, height_pt) = page.render_dimensions();
            let scale = size / width_pt.max(height_pt).max(1.0);
            let data_url = draw(page, &cache, &settings, scale)
                .and_then(|pixmap| pixmap.into_png().ok())
                .map(|png| format!("data:image/png;base64,{}", base64(&png)))
                .unwrap_or_default();
            Thumbnail {
                number,
                width_pt,
                height_pt,
                data_url,
            }
        })
        .collect())
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
    /// Some pages of a PDF file, in this order and each with its turn.
    Pages(Vec<u8>, Vec<PdfPage>),
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
    let mut writer = Writer::new();
    let mut report = AssembleReport::default();
    for part in parts {
        match part {
            Part::Picture {
                jpeg,
                width,
                height,
                placement,
            } => writer.add_picture(jpeg, width, height, placement),
            Part::Document(data) => {
                let source = Source::open(&data)?;
                writer.add_pages(&source, &source.every_page())?;
                report.dropped_extras |= source.extras;
            }
            Part::Pages(data, pages) => {
                let source = Source::open(&data)?;
                writer.add_pages(&source, &pages)?;
                report.dropped_extras |= source.extras;
            }
        }
    }
    report.pages = writer.kids.len() as u32;
    Ok((writer.finish()?, report))
}

/// Makes new PDFs out of the `pages` of one document (in this order, each
/// with its turn): `parts` is the number of pages of each new document (empty:
/// a single one). Each document is handed to `on_document` as soon as it is
/// ready; stops when `cancel` is set or `on_document` returns `false`.
/// Blocking: run it on a worker thread.
pub fn edit(
    data: &[u8],
    pages: &[PdfPage],
    parts: &[u32],
    cancel: &AtomicBool,
    mut on_document: impl FnMut(usize, Vec<u8>) -> bool,
) -> Result<(), CoreError> {
    let source = Source::open(data)?;
    let whole = [pages.len() as u32];
    let sizes = if parts.is_empty() { &whole[..] } else { parts };
    if sizes.iter().map(|&n| n as usize).sum::<usize>() != pages.len() || sizes.contains(&0) {
        return Err(CoreError::InvalidPages {
            details: "the documents do not add up to the chosen pages".into(),
        });
    }
    let mut start = 0;
    for (i, &size) in sizes.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        let mut writer = Writer::new();
        writer.add_pages(&source, &pages[start..start + size as usize])?;
        start += size as usize;
        if !on_document(i, writer.finish()?) {
            return Err(CoreError::Cancelled);
        }
    }
    Ok(())
}

/// Page attributes a page may inherit from its parents.
const INHERITED: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];
/// Page entries that are not copied: the old page tree, and data about the
/// file's history (dates, metadata, private data of the program that made it).
const PAGE_ENTRIES_DROPPED: [&[u8]; 4] = [b"Parent", b"Metadata", b"PieceInfo", b"LastModified"];

/// A PDF opened to copy pages out of it.
struct Source {
    doc: Document,
    /// Page objects, in document order.
    pages: Vec<ObjectId>,
    /// Destinations of the document by name, used by its links.
    named: HashMap<Vec<u8>, Object>,
    /// Bookmarks or form fields, which are not carried over.
    extras: bool,
}

impl Source {
    fn open(data: &[u8]) -> Result<Self, CoreError> {
        // hayro checks the file first: it tells a password-protected PDF apart.
        open(data.to_vec())?;
        let mut doc = Document::load_mem(data).map_err(|e| match e {
            lopdf::Error::Decryption(_) => CoreError::PdfEncrypted,
            other => damaged(&other.to_string()),
        })?;
        if doc.is_encrypted() {
            doc.decrypt("").map_err(|_| CoreError::PdfEncrypted)?;
        }
        let extras = doc
            .catalog()
            .is_ok_and(|c| c.has(b"Outlines") || c.has(b"AcroForm"));
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
        }
        let named = named_destinations(&doc);
        Ok(Self {
            doc,
            pages,
            named,
            extras,
        })
    }

    fn every_page(&self) -> Vec<PdfPage> {
        (1..=self.pages.len() as u32)
            .map(|number| PdfPage {
                number,
                rotation: 0,
            })
            .collect()
    }

    /// Follows references until a direct object.
    fn resolve<'a>(&'a self, mut object: &'a Object) -> &'a Object {
        for _ in 0..32 {
            match object {
                Object::Reference(id) => match self.doc.get_object(*id) {
                    Ok(next) => object = next,
                    Err(_) => return &Object::Null,
                },
                _ => return object,
            }
        }
        &Object::Null
    }
}

/// Named destinations of a document: the `Dests` dictionary of the catalog
/// and the `Dests` name tree, each name mapped to its `[page …]` array.
fn named_destinations(doc: &Document) -> HashMap<Vec<u8>, Object> {
    let mut named = HashMap::new();
    let Ok(catalog) = doc.catalog() else {
        return named;
    };
    let deref = |object: &Object| -> Option<Object> {
        let mut object = object.clone();
        for _ in 0..32 {
            match object {
                Object::Reference(id) => object = doc.get_object(id).ok()?.clone(),
                other => return Some(other),
            }
        }
        None
    };
    // A destination is an array, or a dictionary whose `D` is the array.
    let explicit = |object: &Object| -> Option<Object> {
        match deref(object)? {
            Object::Dictionary(d) => deref(d.get(b"D").ok()?),
            other => Some(other),
        }
        .filter(|o| o.as_array().is_ok())
    };
    if let Some(Object::Dictionary(dests)) = catalog.get(b"Dests").ok().and_then(deref) {
        for (name, value) in dests.iter() {
            if let Some(dest) = explicit(value) {
                named.insert(name.clone(), dest);
            }
        }
    }
    let tree = catalog
        .get(b"Names")
        .ok()
        .and_then(deref)
        .and_then(|n| n.as_dict().ok()?.get(b"Dests").ok().and_then(deref));
    let mut nodes: Vec<(Object, u32)> = tree.into_iter().map(|t| (t, 0)).collect();
    let mut visited = 0;
    while let Some((node, depth)) = nodes.pop() {
        visited += 1;
        let Ok(node) = node.as_dict() else { continue };
        if depth > 32 || visited > 100_000 {
            break;
        }
        if let Some(Object::Array(kids)) = node.get(b"Kids").ok().and_then(deref) {
            nodes.extend(kids.iter().filter_map(|k| deref(k).map(|k| (k, depth + 1))));
        }
        if let Some(Object::Array(pairs)) = node.get(b"Names").ok().and_then(deref) {
            for pair in pairs.chunks(2) {
                if let [Object::String(name, _), value] = pair
                    && let Some(dest) = explicit(value)
                {
                    named.insert(name.clone(), dest);
                }
            }
        }
    }
    named
}

/// A PDF being written, page by page.
struct Writer {
    out: Document,
    pages_id: ObjectId,
    kids: Vec<Object>,
}

impl Writer {
    fn new() -> Self {
        let mut out = Document::with_version("1.7");
        let pages_id = out.new_object_id();
        Self {
            out,
            pages_id,
            kids: Vec::new(),
        }
    }

    fn add_picture(&mut self, jpeg: Vec<u8>, width: u32, height: u32, placement: Placement) {
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
        let image_id = self.out.add_object(image);
        let content = format!(
            "q {:.3} 0 0 {:.3} {:.3} {:.3} cm /Im0 Do Q",
            placement.width, placement.height, placement.x, placement.y
        );
        let mut content = Stream::new(Dictionary::new(), content.into_bytes());
        let _ = content.compress();
        let content_id = self.out.add_object(content);
        let page = dictionary! {
            "Type" => "Page",
            "Parent" => self.pages_id,
            "MediaBox" => vec![0.into(), 0.into(), placement.page_width.into(), placement.page_height.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
            "Contents" => content_id,
        };
        self.kids.push(self.out.add_object(page).into());
    }

    /// Copies `pages` of `source`, in order and turned, with what they use
    /// (contents, fonts, pictures, annotations): the other pages, the
    /// document structure and its metadata stay behind. Links to pages that
    /// are left out are removed.
    fn add_pages(&mut self, source: &Source, pages: &[PdfPage]) -> Result<(), CoreError> {
        let doc = &source.doc;
        let chosen: Vec<ObjectId> = pages
            .iter()
            .map(|page| {
                (page.number as usize)
                    .checked_sub(1)
                    .and_then(|i| source.pages.get(i))
                    .copied()
                    .ok_or_else(|| CoreError::InvalidPages {
                        details: format!("page {} does not exist", page.number),
                    })
            })
            .collect::<Result<_, _>>()?;
        let kept: HashSet<ObjectId> = chosen.iter().copied().collect();
        if kept.len() != chosen.len() {
            return Err(CoreError::InvalidPages {
                details: "a page is listed twice".into(),
            });
        }
        let every_page: HashSet<ObjectId> = source.pages.iter().copied().collect();

        // Everything reachable from the chosen pages, except other pages and
        // document-level objects. New numbers avoid clashes with this writer's.
        let mut numbers: HashMap<ObjectId, ObjectId> = HashMap::new();
        let mut queue: VecDeque<ObjectId> = VecDeque::new();
        for &id in &chosen {
            numbers.insert(id, self.out.new_object_id());
            queue.push_back(id);
        }
        let mut found = Vec::new();
        while let Some(id) = queue.pop_front() {
            let Ok(object) = doc.get_object(id) else {
                continue;
            };
            found.clear();
            references(object, kept.contains(&id), &mut found);
            if let Some(dest) = named_target(source, object) {
                references(dest, false, &mut found);
            }
            for &r in &found {
                if numbers.contains_key(&r) || every_page.contains(&r) {
                    continue;
                }
                if doc.get_object(r).is_ok_and(|o| !document_level(o)) {
                    numbers.insert(r, self.out.new_object_id());
                    queue.push_back(r);
                }
            }
        }

        for (&old, &new) in &numbers {
            let Ok(object) = doc.get_object(old) else {
                continue;
            };
            let mut object = object.clone();
            if kept.contains(&old)
                && let Object::Dictionary(dict) = &mut object
            {
                for key in PAGE_ENTRIES_DROPPED {
                    dict.remove(key);
                }
            }
            // A link to a named destination points to its page directly,
            // since the names of the document are not kept.
            if let Some(dest) = named_target(source, &object).cloned()
                && let Object::Dictionary(dict) = &mut object
            {
                let key: &[u8] = if dict.has(b"Dest") { b"Dest" } else { b"D" };
                dict.set(key, dest);
            }
            renumber(&mut object, &numbers);
            self.out.objects.insert(new, object);
        }

        for (page, old) in pages.iter().zip(&chosen) {
            let id = numbers[old];
            let dict = self
                .out
                .get_dictionary_mut(id)
                .map_err(|e| damaged(&e.to_string()))?;
            dict.set("Parent", self.pages_id);
            let turn = i64::from(page.rotation % 360);
            if turn != 0 {
                let own = dict.get(b"Rotate").and_then(Object::as_i64).unwrap_or(0);
                dict.set("Rotate", (own + turn).rem_euclid(360));
            }
            self.drop_dead_links(id);
            self.kids.push(id.into());
        }
        Ok(())
    }

    /// Removes the links of a page that lead nowhere (to a page left out).
    fn drop_dead_links(&mut self, page: ObjectId) {
        let out = &self.out;
        let resolve = |object: &Object| -> Option<Object> {
            let mut object = object.clone();
            for _ in 0..8 {
                match object {
                    Object::Reference(id) => object = out.get_object(id).ok()?.clone(),
                    other => return Some(other),
                }
            }
            None
        };
        let dead = |annotation: &Object| -> bool {
            let Some(Object::Dictionary(link)) = resolve(annotation) else {
                return false;
            };
            if !link
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_ok_and(|s| s == b"Link")
            {
                return false;
            }
            let dest = match link.get(b"Dest") {
                Ok(dest) => resolve(dest),
                Err(_) => match link.get(b"A").ok().and_then(resolve) {
                    Some(Object::Dictionary(action))
                        if action
                            .get(b"S")
                            .and_then(Object::as_name)
                            .is_ok_and(|s| s == b"GoTo") =>
                    {
                        action.get(b"D").ok().and_then(resolve)
                    }
                    _ => return false,
                },
            };
            match dest {
                Some(Object::Array(items)) => !matches!(items.first(), Some(Object::Reference(_))),
                // A name that could not be found among the document's destinations.
                Some(Object::Name(_) | Object::String(..)) => true,
                _ => false,
            }
        };
        let Ok(annots) = out.get_dictionary(page).and_then(|p| p.get(b"Annots")) else {
            return;
        };
        let Some(Object::Array(list)) = resolve(annots) else {
            return;
        };
        if !list.iter().any(dead) {
            return;
        }
        let alive: Vec<Object> = list.iter().filter(|a| !dead(a)).cloned().collect();
        if let Ok(dict) = self.out.get_dictionary_mut(page) {
            dict.set("Annots", alive);
        }
    }

    fn finish(mut self) -> Result<Vec<u8>, CoreError> {
        let count = self.kids.len() as i64;
        self.out.objects.insert(
            self.pages_id,
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => self.kids, "Count" => count },
            ),
        );
        let catalog_id = self
            .out
            .add_object(dictionary! { "Type" => "Catalog", "Pages" => self.pages_id });
        let info_id = self
            .out
            .add_object(dictionary! { "Producer" => Object::string_literal("Konvert") });
        self.out.trailer.set("Root", catalog_id);
        self.out.trailer.set("Info", info_id);
        self.out.compress();
        let mut bytes = Vec::new();
        self.out
            .save_to(&mut bytes)
            .map_err(|e| CoreError::Io(std::io::Error::other(e.to_string())))?;
        Ok(bytes)
    }
}

/// Objects that belong to the document as a whole, never copied with a page.
fn document_level(object: &Object) -> bool {
    let dict = match object {
        Object::Dictionary(d) => d,
        Object::Stream(s) => &s.dict,
        _ => return false,
    };
    matches!(
        dict.get(b"Type").and_then(Object::as_name),
        Ok(b"Catalog" | b"Pages" | b"Outlines" | b"Outline" | b"Metadata" | b"ObjStm" | b"XRef")
    )
}

/// The page array of a link (or `GoTo` action) whose destination is a name.
fn named_target<'a>(source: &'a Source, object: &Object) -> Option<&'a Object> {
    let dict = object.as_dict().ok()?;
    let name = match dict.get(b"Dest") {
        Ok(dest) => dest,
        Err(_)
            if dict
                .get(b"S")
                .and_then(Object::as_name)
                .is_ok_and(|s| s == b"GoTo") =>
        {
            dict.get(b"D").ok()?
        }
        Err(_) => return None,
    };
    let name = match source.resolve(name) {
        Object::Name(n) | Object::String(n, _) => n,
        _ => return None,
    };
    source.named.get(name)
}

/// References held by an object; for a copied page, not those of the entries
/// that are dropped.
fn references(object: &Object, page: bool, found: &mut Vec<ObjectId>) {
    match object {
        Object::Reference(id) => found.push(*id),
        Object::Array(items) => items.iter().for_each(|o| references(o, false, found)),
        Object::Dictionary(dict) => {
            for (key, value) in dict.iter() {
                if !(page && PAGE_ENTRIES_DROPPED.contains(&key.as_slice())) {
                    references(value, false, found);
                }
            }
        }
        Object::Stream(stream) => {
            for (_, value) in stream.dict.iter() {
                references(value, false, found);
            }
        }
        _ => {}
    }
}

/// Gives references their new numbers; those to objects left behind become null.
fn renumber(object: &mut Object, numbers: &HashMap<ObjectId, ObjectId>) {
    match object {
        Object::Reference(id) => {
            let new = numbers
                .get(id)
                .map_or(Object::Null, |n| Object::Reference(*n));
            *object = new;
        }
        Object::Array(items) => items.iter_mut().for_each(|o| renumber(o, numbers)),
        Object::Dictionary(dict) => dict.iter_mut().for_each(|(_, o)| renumber(o, numbers)),
        Object::Stream(stream) => stream
            .dict
            .iter_mut()
            .for_each(|(_, o)| renumber(o, numbers)),
        _ => {}
    }
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

    fn page(number: u32, rotation: u16) -> PdfPage {
        PdfPage { number, rotation }
    }

    /// Edits `data` into a single document.
    fn edit_one(data: &[u8], pages: &[PdfPage]) -> Vec<u8> {
        let mut result = Vec::new();
        edit(data, pages, &[], &AtomicBool::new(false), |_, bytes| {
            result.push(bytes);
            true
        })
        .unwrap();
        assert_eq!(result.len(), 1);
        result.pop().unwrap()
    }

    fn rotations(data: &[u8]) -> Vec<i64> {
        let doc = Document::load_mem(data).unwrap();
        doc.get_pages()
            .values()
            .map(|id| {
                let page = doc.get_dictionary(*id).unwrap();
                page.get(b"Rotate").and_then(Object::as_i64).unwrap_or(0)
            })
            .collect()
    }

    /// Three A4 pages sharing a font; page 1 links to page 2 (explicit
    /// destination), to page 3 (named destination) and to page 3 again (a
    /// `GoTo` action). The document has a bookmark.
    fn linked_pdf() -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let ids: Vec<ObjectId> = (0..3).map(|_| doc.new_object_id()).collect();
        let rect = || -> Vec<Object> { vec![0.into(), 0.into(), 10.into(), 10.into()] };
        for (i, &id) in ids.iter().enumerate() {
            let text = format!("BT /F1 24 Tf 72 720 Td (Pagina {}) Tj ET", i + 1);
            let content = doc.add_object(Stream::new(Dictionary::new(), text.into_bytes()));
            let mut page = dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Contents" => content,
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
                "LastModified" => Object::string_literal("D:20260101120000"),
            };
            if i == 0 {
                let explicit = doc.add_object(dictionary! {
                    "Type" => "Annot", "Subtype" => "Link", "Rect" => rect(),
                    "Dest" => vec![ids[1].into(), "Fit".into()],
                });
                let named = doc.add_object(dictionary! {
                    "Type" => "Annot", "Subtype" => "Link", "Rect" => rect(),
                    "Dest" => Object::string_literal("terza"),
                });
                let action = doc.add_object(dictionary! {
                    "Type" => "Annot", "Subtype" => "Link", "Rect" => rect(),
                    "A" => dictionary! { "S" => "GoTo", "D" => vec![ids[2].into(), "Fit".into()] },
                });
                page.set("Annots", vec![explicit.into(), named.into(), action.into()]);
            }
            doc.objects.insert(id, Object::Dictionary(page));
        }
        let kids: Vec<Object> = ids.iter().map(|id| (*id).into()).collect();
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 3 }),
        );
        let outlines = doc.new_object_id();
        let item = doc.add_object(dictionary! {
            "Title" => Object::string_literal("Inizio"),
            "Parent" => outlines,
            "Dest" => vec![ids[0].into(), "Fit".into()],
        });
        doc.objects.insert(
            outlines,
            Object::Dictionary(dictionary! {
                "Type" => "Outlines", "First" => item, "Last" => item, "Count" => 1,
            }),
        );
        let names = dictionary! {
            "Dests" => dictionary! {
                "Names" => vec![Object::string_literal("terza"), vec![ids[2].into(), "Fit".into()].into()],
            },
        };
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => pages_id, "Outlines" => outlines, "Names" => names,
        });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    /// Links of the first page of `data`: where each one leads (index of the
    /// target page in the document).
    fn link_targets(data: &[u8]) -> Vec<Option<usize>> {
        let doc = Document::load_mem(data).unwrap();
        let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
        let first = doc.get_dictionary(pages[0]).unwrap();
        let Ok(annots) = first.get(b"Annots").and_then(Object::as_array) else {
            return Vec::new();
        };
        annots
            .iter()
            .map(|a| {
                let link = doc.get_dictionary(a.as_reference().unwrap()).unwrap();
                let dest = match link.get(b"Dest") {
                    Ok(dest) => dest.clone(),
                    Err(_) => {
                        let action = link.get(b"A").unwrap();
                        let action = match action {
                            Object::Reference(id) => doc.get_dictionary(*id).unwrap(),
                            other => other.as_dict().unwrap(),
                        };
                        action.get(b"D").unwrap().clone()
                    }
                };
                let target = dest.as_array().unwrap()[0].as_reference().ok()?;
                pages.iter().position(|p| *p == target)
            })
            .collect()
    }

    #[test]
    fn layouts_are_checked() {
        assert_eq!(
            check_layout(None, 3).unwrap(),
            [page(1, 0), page(2, 0), page(3, 0)]
        );
        assert_eq!(
            check_layout(Some(&[page(3, 450), page(1, 0)]), 3).unwrap(),
            [page(3, 90), page(1, 0)]
        );
        for wrong in [
            vec![],
            vec![page(0, 0)],
            vec![page(4, 0)],
            vec![page(2, 0), page(2, 90)],
            vec![page(1, 45)],
        ] {
            let err = check_layout(Some(&wrong), 3).unwrap_err();
            assert_eq!(err.code(), "invalid_pages", "{wrong:?}");
        }
    }

    #[test]
    fn documents_are_split_every_few_pages_or_at_chosen_places() {
        assert!(split_sizes(PdfSplit::None, 2, &[2], 5).unwrap().is_empty());
        assert_eq!(split_sizes(PdfSplit::Every, 2, &[], 5).unwrap(), [2, 2, 1]);
        assert_eq!(split_sizes(PdfSplit::Every, 1, &[], 3).unwrap(), [1, 1, 1]);
        // Every page in one document: no split at all.
        assert!(split_sizes(PdfSplit::Every, 5, &[], 5).unwrap().is_empty());
        assert_eq!(
            split_sizes(PdfSplit::Every, 0, &[], 5).unwrap_err().code(),
            "invalid_pages"
        );
        // Cuts out of the document (left over from removed pages) are ignored.
        assert_eq!(
            split_sizes(PdfSplit::After, 1, &[3, 1, 9, 3, 0, 5], 5).unwrap(),
            [1, 2, 2]
        );
        assert!(split_sizes(PdfSplit::After, 1, &[], 5).unwrap().is_empty());
    }

    #[test]
    fn pages_are_reordered_turned_and_removed() {
        let mut doc = Document::load_mem(&sample_pdf(&[
            (100.0, 200.0),
            (300.0, 400.0),
            (500.0, 600.0),
        ]))
        .unwrap();
        // Every page inherits a quarter turn from the page tree.
        let tree = doc
            .catalog()
            .unwrap()
            .get(b"Pages")
            .unwrap()
            .as_reference()
            .unwrap();
        doc.get_dictionary_mut(tree).unwrap().set("Rotate", 90);
        let mut data = Vec::new();
        doc.save_to(&mut data).unwrap();

        let edited = edit_one(&data, &[page(3, 0), page(1, 270)]);
        assert_eq!(rotations(&edited), [90, 0]);
        let sizes: Vec<(f32, f32)> = open(edited.clone())
            .unwrap()
            .pages()
            .iter()
            .map(|p| p.render_dimensions())
            .collect();
        assert_eq!(sizes, [(600.0, 500.0), (100.0, 200.0)]);
        // No personal metadata of the source.
        let text = String::from_utf8_lossy(&edited);
        assert!(!text.contains("Mario Rossi") && !text.contains("D:2026"));
        // Merging honours the chosen pages too.
        let (merged, report) = assemble(vec![
            Part::Pages(data.clone(), vec![page(2, 180)]),
            Part::Document(data),
        ])
        .unwrap();
        assert_eq!(report.pages, 4);
        assert_eq!(rotations(&merged), [270, 90, 90, 90]);
    }

    #[test]
    fn extracted_pages_leave_the_rest_of_the_document_behind() {
        // Four pages, each with its own picture of noise (it does not compress).
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let mut seed = 7u32;
        let mut kids = Vec::new();
        for _ in 0..4 {
            let noise: Vec<u8> = (0..120_000)
                .map(|_| {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    (seed >> 16) as u8
                })
                .collect();
            let image = doc.add_object(Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Image", "Width" => 200, "Height" => 200,
                    "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
                },
                noise,
            ));
            let content = doc.add_object(Stream::new(
                Dictionary::new(),
                b"q 200 0 0 200 0 0 cm /Im0 Do Q".to_vec(),
            ));
            let page = doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } },
                "Contents" => content,
            });
            kids.push(Object::from(page));
        }
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 4 }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut data = Vec::new();
        doc.save_to(&mut data).unwrap();
        assert!(data.len() > 480_000);

        let one = edit_one(&data, &[page(2, 0)]);
        assert_eq!(page_count(&one).unwrap(), 1);
        assert!(one.len() > 120_000 && one.len() < 180_000, "{}", one.len());
        let mut seen = Vec::new();
        render_pages(one, &[1], 72, &AtomicBool::new(false), |p| {
            seen.push(p.png.len() > 1000);
            true
        })
        .unwrap();
        assert_eq!(seen, [true]);
    }

    #[test]
    fn links_follow_their_pages_and_links_to_removed_pages_go() {
        let data = linked_pdf();
        assert!(has_extras(&open(data.clone()).unwrap()));
        assert!(Source::open(&data).unwrap().extras);

        // All pages: every link still works, the named one now points directly.
        let all = edit_one(&data, &[page(1, 0), page(2, 0), page(3, 0)]);
        assert_eq!(link_targets(&all), [Some(1), Some(2), Some(2)]);
        // Page 2 is gone: so is the link to it.
        let without_second = edit_one(&data, &[page(1, 0), page(3, 0)]);
        assert_eq!(link_targets(&without_second), [Some(1), Some(1)]);
        // Page 3 is gone: only the link to page 2 is left.
        let without_third = edit_one(&data, &[page(1, 0), page(2, 0)]);
        assert_eq!(link_targets(&without_third), [Some(1)]);

        // Bookmarks, names and page dates are not carried over; the font is shared.
        let doc = Document::load_mem(&without_second).unwrap();
        let catalog = doc.catalog().unwrap();
        assert!(!catalog.has(b"Outlines") && !catalog.has(b"Names"));
        assert!(!has_extras(&open(without_second.clone()).unwrap()));
        let text = String::from_utf8_lossy(&without_second);
        assert!(!text.contains("D:2026"));
        let fonts = doc
            .objects
            .values()
            .filter(|o| o.as_dict().is_ok_and(|d| d.has_type(b"Font")))
            .count();
        assert_eq!(fonts, 1);
    }

    #[test]
    fn a_document_is_split_into_parts_and_can_be_stopped() {
        let data = sample_pdf(&[(A4.0, A4.1); 5]);
        let pages: Vec<PdfPage> = (1..=5).map(|n| page(n, 0)).collect();
        let mut counts = Vec::new();
        edit(
            &data,
            &pages,
            &[2, 3],
            &AtomicBool::new(false),
            |i, bytes| {
                counts.push((i, page_count(&bytes).unwrap()));
                true
            },
        )
        .unwrap();
        assert_eq!(counts, [(0, 2), (1, 3)]);
        // Parts that do not match the pages.
        let err = edit(&data, &pages, &[2, 2], &AtomicBool::new(false), |_, _| true);
        assert_eq!(err.unwrap_err().code(), "invalid_pages");
        // Stopped after the first part, or before any.
        let err = edit(&data, &pages, &[2, 3], &AtomicBool::new(false), |_, _| {
            false
        });
        assert_eq!(err.unwrap_err().code(), "cancelled");
        let err = edit(&data, &pages, &[2, 3], &AtomicBool::new(true), |_, _| true);
        assert_eq!(err.unwrap_err().code(), "cancelled");
    }

    #[test]
    fn thumbnails_are_small_pictures_of_existing_pages() {
        let data = sample_pdf(&[(A4.0, A4.1), (200.0, 100.0)]);
        let thumbs = thumbnails(data, &[2, 9, 1], 64).unwrap();
        let numbers: Vec<u32> = thumbs.iter().map(|t| t.number).collect();
        assert_eq!(numbers, [2, 1]);
        assert_eq!((thumbs[0].width_pt, thumbs[0].height_pt), (200.0, 100.0));
        for thumb in &thumbs {
            assert!(
                thumb
                    .data_url
                    .starts_with("data:image/png;base64,iVBORw0KGgo")
            );
            assert!(thumb.data_url.len() < 20_000);
        }
        assert_eq!(
            thumbnails(b"%PDF-1.7 nonsense".to_vec(), &[1], 64)
                .unwrap_err()
                .code(),
            "corrupt_input"
        );
    }
}
