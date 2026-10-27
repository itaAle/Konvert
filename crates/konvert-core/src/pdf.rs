//! PDF documents, handled without FFmpeg:
//!
//! * analysis ([`probe`]) and rendering of pages to pictures and thumbnails
//!   with hayro (pure Rust, no unsafe code);
//! * pictures laid out on pages ([`place`]), and new documents built from
//!   the pages of several files, in any order, turned, split into several
//!   documents ([`build`]) with lopdf. Bookmarks and fillable forms come along
//!   with their pages.
//!
//! Konvert never asks for passwords: a PDF that needs one gives
//! [`CoreError::PdfEncrypted`]. PDFs protected only against editing (empty
//! user password) open normally. The PDFs Konvert writes carry no path, user
//! name or date: their only metadata is `Producer: Konvert`.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::object::{self as hayro_object, Array, Dict, Name, ObjectIdentifier};
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
    /// Pages the bookmarks lead to (1-based, in order, without repeats).
    #[serde(default)]
    pub bookmark_pages: Vec<u32>,
    #[serde(default)]
    pub form: PdfForm,
}

/// The fillable form of a PDF, as the PDFs made from it keep it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PdfForm {
    /// No fillable field.
    #[default]
    None,
    /// Fields that stay fillable.
    Fillable,
    /// An XFA form, or one with a digital signature: its fields keep their
    /// look but can no longer be filled in.
    Fixed,
}

/// Most bookmarks, form fields or name tree nodes read from one document.
const MAX_ITEMS: usize = 100_000;
/// Deepest bookmark or field tree followed.
const MAX_DEPTH: u32 = 64;

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
    let (bookmark_pages, form) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (bookmark_pages(&pdf), form_kind(&pdf))
    }))
    .unwrap_or_default();
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
            bookmark_pages,
            form,
        }),
        text: None,
    })
}

/// Pages the bookmarks of a document lead to (1-based, sorted, without repeats).
fn bookmark_pages(pdf: &Pdf) -> Vec<u32> {
    let xref = pdf.xref();
    let Some(catalog) = xref.get::<Dict>(xref.root_id()) else {
        return Vec::new();
    };
    let Some(first) = catalog
        .get::<Dict>(b"Outlines")
        .and_then(|o| o.get::<Dict>(b"First"))
    else {
        return Vec::new();
    };
    let numbers: HashMap<ObjectIdentifier, u32> = pdf
        .pages()
        .iter()
        .enumerate()
        .filter_map(|(i, page)| Some((page.raw().obj_id()?, i as u32 + 1)))
        .collect();
    let mut named = None;
    let mut found = BTreeSet::new();
    let mut seen = HashSet::new();
    let mut stack = vec![first];
    let mut visited = 0;
    while let Some(item) = stack.pop() {
        visited += 1;
        if visited > MAX_ITEMS || item.obj_id().is_some_and(|id| !seen.insert(id)) {
            continue;
        }
        let dest = match item.get::<hayro_object::Object>(b"Dest") {
            Some(dest) => Some(dest),
            None => item
                .get::<Dict>(b"A")
                .filter(|a| a.get::<Name>(b"S").is_some_and(|s| &*s == b"GoTo"))
                .and_then(|a| a.get::<hayro_object::Object>(b"D")),
        };
        if let Some(page) = dest
            .and_then(|d| destination_page(&catalog, d, &mut named))
            .and_then(|id| numbers.get(&id))
        {
            found.insert(*page);
        }
        stack.extend(item.get::<Dict>(b"Next"));
        stack.extend(item.get::<Dict>(b"First"));
    }
    found.into_iter().collect()
}

/// The page object a destination leads to: an explicit `[page …]` array, a
/// dictionary holding one, or a name looked up among the document's
/// destinations (read once, when first needed).
fn destination_page<'a>(
    catalog: &Dict<'a>,
    dest: hayro_object::Object<'a>,
    named: &mut Option<HashMap<Vec<u8>, ObjectIdentifier>>,
) -> Option<ObjectIdentifier> {
    use hayro_object::Object as O;
    match dest {
        O::Array(array) => array.raw_iter().next()?.as_obj_ref().map(Into::into),
        O::Dict(dict) => match dict.get::<hayro_object::Object>(b"D")? {
            O::Array(array) => array.raw_iter().next()?.as_obj_ref().map(Into::into),
            _ => None,
        },
        O::Name(name) => named
            .get_or_insert_with(|| named_pages(catalog))
            .get(&*name)
            .copied(),
        O::String(name) => named
            .get_or_insert_with(|| named_pages(catalog))
            .get(name.as_bytes())
            .copied(),
        _ => None,
    }
}

/// The page of each named destination: the `Dests` dictionary of the
/// catalog and the `Dests` name tree.
fn named_pages(catalog: &Dict<'_>) -> HashMap<Vec<u8>, ObjectIdentifier> {
    use hayro_object::Object as O;
    let page_of = |value: O<'_>| -> Option<ObjectIdentifier> {
        let array = match value {
            O::Array(array) => array,
            O::Dict(dict) => dict.get::<Array>(b"D")?,
            _ => return None,
        };
        array.raw_iter().next()?.as_obj_ref().map(Into::into)
    };
    let mut named = HashMap::new();
    if let Some(dests) = catalog.get::<Dict>(b"Dests") {
        for key in dests.keys() {
            if let Some(page) = dests.get::<O>(&*key).and_then(page_of) {
                named.insert(key.to_vec(), page);
            }
        }
    }
    let tree = catalog
        .get::<Dict>(b"Names")
        .and_then(|n| n.get::<Dict>(b"Dests"));
    let mut nodes: Vec<(Dict, u32)> = tree.into_iter().map(|t| (t, 0)).collect();
    let mut visited = 0;
    while let Some((node, depth)) = nodes.pop() {
        visited += 1;
        if depth > MAX_DEPTH || visited > MAX_ITEMS {
            break;
        }
        if let Some(kids) = node.get::<Array>(b"Kids") {
            nodes.extend(kids.iter::<Dict>().map(|k| (k, depth + 1)));
        }
        if let Some(pairs) = node.get::<Array>(b"Names") {
            let mut items = pairs.iter::<O>();
            while let (Some(name), Some(value)) = (items.next(), items.next()) {
                if let (O::String(name), Some(page)) = (name, page_of(value)) {
                    named.insert(name.as_bytes().to_vec(), page);
                }
            }
        }
    }
    named
}

/// The kind of fillable form of a document.
fn form_kind(pdf: &Pdf) -> PdfForm {
    let xref = pdf.xref();
    let Some(form) = xref
        .get::<Dict>(xref.root_id())
        .and_then(|c| c.get::<Dict>(b"AcroForm"))
    else {
        return PdfForm::None;
    };
    let Some(fields) = form.get::<Array>(b"Fields") else {
        return PdfForm::None;
    };
    let mut nodes: Vec<(Dict, u32)> = fields.iter::<Dict>().map(|f| (f, 0)).collect();
    if nodes.is_empty() {
        return PdfForm::None;
    }
    if form.contains_key(b"XFA") {
        return PdfForm::Fixed;
    }
    let mut visited = 0;
    while let Some((field, depth)) = nodes.pop() {
        visited += 1;
        if depth > MAX_DEPTH || visited > MAX_ITEMS {
            break;
        }
        let signed =
            field.get::<Name>(b"FT").is_some_and(|t| &*t == b"Sig") && field.contains_key(b"V");
        if signed {
            return PdfForm::Fixed;
        }
        if let Some(kids) = field.get::<Array>(b"Kids") {
            nodes.extend(kids.iter::<Dict>().map(|k| (k, depth + 1)));
        }
    }
    PdfForm::Fillable
}

/// Pages of a PDF made from another one: `None` means every page as it is.
/// Each page appears at most once; turns are multiples of 90 degrees.
pub fn check_layout(layout: Option<&[PdfPage]>, count: u32) -> Result<Vec<PdfPage>, CoreError> {
    check_pages(layout, &[count])
}

/// Pages of a PDF made from several files (`counts`: pages of each file, a
/// picture has one): `None` means every page of every file, in order. Each
/// page appears at most once; turns are multiples of 90 degrees.
pub fn check_pages(layout: Option<&[PdfPage]>, counts: &[u32]) -> Result<Vec<PdfPage>, CoreError> {
    let Some(layout) = layout else {
        return Ok(counts
            .iter()
            .enumerate()
            .flat_map(|(file, &count)| {
                (1..=count).map(move |number| PdfPage {
                    file: file as u32,
                    number,
                    rotation: 0,
                })
            })
            .collect());
    };
    let invalid = |details: String| CoreError::InvalidPages { details };
    if layout.is_empty() {
        return Err(invalid("no page left in the document".into()));
    }
    let mut seen = HashSet::new();
    layout
        .iter()
        .map(|page| {
            let Some(&count) = counts.get(page.file as usize) else {
                return Err(invalid(format!("file {} does not exist", page.file + 1)));
            };
            if page.number == 0 || page.number > count {
                return Err(invalid(format!(
                    "page {} does not exist (the document has {count} pages)",
                    page.number
                )));
            }
            if !seen.insert((page.file, page.number)) {
                return Err(invalid(format!("page {} is listed twice", page.number)));
            }
            if page.rotation % 90 != 0 {
                return Err(invalid(format!("invalid turn {}°", page.rotation)));
            }
            Ok(PdfPage {
                rotation: page.rotation % 360,
                ..*page
            })
        })
        .collect()
}

/// The pages of one file in a layout of several, as a layout of that file alone.
pub fn pages_of_file(layout: &[PdfPage], file: u32) -> Vec<PdfPage> {
    layout
        .iter()
        .filter(|p| p.file == file)
        .map(|p| PdfPage { file: 0, ..*p })
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
pub(crate) fn draw<'a>(
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

// --- Building documents ------------------------------------------------------------

/// One file of a new document.
pub enum Part {
    /// A JPEG picture (RGB) of `width`×`height` pixels, on a page of its own.
    Picture {
        jpeg: Vec<u8>,
        width: u32,
        height: u32,
        placement: Placement,
    },
    /// A PDF file.
    Document(Vec<u8>),
}

/// Builds one PDF from pictures and documents, every page in order. Gives
/// the document and its number of pages.
pub fn assemble(parts: Vec<Part>) -> Result<(Vec<u8>, u32), CoreError> {
    let mut document = Vec::new();
    let pages = build(parts, None, &[], &AtomicBool::new(false), |_, bytes| {
        document = bytes;
        true
    })?;
    Ok((document, pages))
}

/// Makes new PDFs out of the `pages` of one document (in this order, each
/// with its turn): see [`build`].
pub fn edit(
    data: Vec<u8>,
    pages: &[PdfPage],
    parts: &[u32],
    cancel: &AtomicBool,
    on_document: impl FnMut(usize, Vec<u8>) -> bool,
) -> Result<(), CoreError> {
    build(
        vec![Part::Document(data)],
        Some(pages),
        parts,
        cancel,
        on_document,
    )
    .map(|_| ())
}

/// Makes new PDFs out of the pages of `files`: `layout` lists them in order,
/// each with its turn (`None`: every page of every file), and `parts` is the
/// number of pages of each new document (empty: a single one). Bookmarks and
/// fillable forms come along with their pages. Each document is handed to
/// `on_document` as soon as it is ready; stops when `cancel` is set or
/// `on_document` returns `false`. Gives the number of pages.
/// Blocking: run it on a worker thread.
pub fn build(
    files: Vec<Part>,
    layout: Option<&[PdfPage]>,
    parts: &[u32],
    cancel: &AtomicBool,
    mut on_document: impl FnMut(usize, Vec<u8>) -> bool,
) -> Result<u32, CoreError> {
    let inputs = files
        .into_iter()
        .map(|file| match file {
            Part::Picture {
                jpeg,
                width,
                height,
                placement,
            } => Ok(Input::Picture {
                jpeg,
                width,
                height,
                placement,
            }),
            Part::Document(data) => Source::open(&data).map(|s| Input::Pdf(Box::new(s))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let counts: Vec<u32> = inputs.iter().map(Input::pages).collect();
    let pages = check_pages(layout, &counts)?;
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
        writer.add(&inputs, &pages[start..start + size as usize])?;
        start += size as usize;
        if !on_document(i, writer.finish(&inputs)?) {
            return Err(CoreError::Cancelled);
        }
    }
    Ok(pages.len() as u32)
}

/// Page attributes a page may inherit from its parents.
const INHERITED: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];
/// Page entries that are not copied: the old page tree, and data about the
/// file's history (dates, metadata, private data of the program that made it).
const PAGE_ENTRIES_DROPPED: [&[u8]; 4] = [b"Parent", b"Metadata", b"PieceInfo", b"LastModified"];

/// A file whose pages go into new documents.
enum Input {
    Picture {
        jpeg: Vec<u8>,
        width: u32,
        height: u32,
        placement: Placement,
    },
    Pdf(Box<Source>),
}

impl Input {
    fn pages(&self) -> u32 {
        match self {
            Input::Picture { .. } => 1,
            Input::Pdf(source) => source.pages.len() as u32,
        }
    }
}

/// A PDF opened to copy pages out of it.
struct Source {
    doc: Document,
    /// Page objects, in document order.
    pages: Vec<ObjectId>,
    /// Position of each page object in `pages`.
    page_index: HashMap<ObjectId, usize>,
    /// Destinations of the document by name, used by its links.
    named: HashMap<Vec<u8>, Object>,
    /// Annotations of each page (those held by reference).
    annots: HashMap<ObjectId, Vec<ObjectId>>,
    /// Every annotation of the document held by reference.
    all_annots: HashSet<ObjectId>,
    outline: Vec<Bookmark>,
    /// The document opens with its bookmarks shown.
    show_outline: bool,
    /// The fillable form, when it can be kept (not XFA, not signed).
    form: Option<Form>,
}

/// One bookmark of a document, with the bookmarks under it.
struct Bookmark {
    title: Object,
    /// The page it leads to, and how it is shown (`/XYZ x y zoom`…).
    target: Option<(ObjectId, Vec<Object>)>,
    /// Any other action (a web link, a named action…), copied as it is.
    action: Option<Object>,
    /// Color and style of the title.
    style: Vec<(&'static [u8], Object)>,
    open: bool,
    children: Vec<Bookmark>,
}

/// The fillable form of a document.
struct Form {
    /// Fields at the top of the form.
    fields: Vec<ObjectId>,
    /// Fonts and other resources of the fields (`DR`).
    resources: Option<Object>,
    /// Default appearance and alignment, appearances to be made (`DA`, `Q`,
    /// `NeedAppearances`).
    entries: Vec<(&'static [u8], Object)>,
    /// Calculation order of the fields (`CO`).
    order: Vec<ObjectId>,
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
        let mut annots = HashMap::new();
        for &page in &pages {
            let list: Vec<ObjectId> = doc
                .get_dictionary(page)
                .and_then(|p| p.get(b"Annots"))
                .map(|a| resolve(&doc, a))
                .and_then(Object::as_array)
                .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
                .unwrap_or_default();
            annots.insert(page, list);
        }
        let mut source = Self {
            page_index: pages.iter().enumerate().map(|(i, &p)| (p, i)).collect(),
            named: named_destinations(&doc),
            all_annots: annots.values().flatten().copied().collect(),
            annots,
            show_outline: doc.catalog().is_ok_and(|c| {
                c.get(b"PageMode")
                    .and_then(Object::as_name)
                    .is_ok_and(|m| m == b"UseOutlines")
            }),
            outline: Vec::new(),
            form: None,
            pages,
            doc,
        };
        source.outline = source.read_outline();
        source.form = source.read_form();
        Ok(source)
    }

    /// Follows references until a direct object.
    fn resolve<'a>(&'a self, object: &'a Object) -> &'a Object {
        resolve(&self.doc, object)
    }

    /// An entry of the catalog, resolved.
    fn catalog_entry(&self, key: &[u8]) -> Option<&Object> {
        let catalog = self.doc.catalog().ok()?;
        Some(self.resolve(catalog.get(key).ok()?))
    }

    fn read_outline(&self) -> Vec<Bookmark> {
        let first = self
            .catalog_entry(b"Outlines")
            .and_then(|o| o.as_dict().ok())
            .and_then(|o| o.get(b"First").ok());
        let mut budget = MAX_ITEMS;
        self.read_bookmarks(first, 0, &mut budget, &mut HashSet::new())
    }

    /// A list of bookmarks, from the first one, with those under them.
    fn read_bookmarks(
        &self,
        first: Option<&Object>,
        depth: u32,
        budget: &mut usize,
        seen: &mut HashSet<ObjectId>,
    ) -> Vec<Bookmark> {
        let mut bookmarks = Vec::new();
        let mut next = first;
        while depth <= MAX_DEPTH
            && let Some(&Object::Reference(id)) = next
            && *budget > 0
            && seen.insert(id)
        {
            *budget -= 1;
            let Ok(item) = self.doc.get_dictionary(id) else {
                break;
            };
            let get = |key: &[u8]| item.get(key).ok().map(|o| self.resolve(o));
            let (target, action) = match (get(b"Dest"), get(b"A")) {
                (Some(dest), _) => (self.destination(dest, 0), None),
                (None, Some(Object::Dictionary(a)))
                    if a.get(b"S")
                        .and_then(Object::as_name)
                        .is_ok_and(|s| s == b"GoTo") =>
                {
                    let dest = a.get(b"D").ok().map(|d| self.resolve(d));
                    (dest.and_then(|d| self.destination(d, 0)), None)
                }
                (None, Some(_)) => (None, item.get(b"A").ok().cloned()),
                (None, None) => (None, None),
            };
            let style = [b"C" as &[u8], b"F"]
                .into_iter()
                .filter_map(|key| Some((key, get(key)?.clone())))
                .collect();
            bookmarks.push(Bookmark {
                title: get(b"Title")
                    .filter(|t| matches!(t, Object::String(..)))
                    .cloned()
                    .unwrap_or_else(|| Object::string_literal("")),
                target,
                action,
                style,
                open: get(b"Count")
                    .and_then(|c| c.as_i64().ok())
                    .is_some_and(|c| c > 0),
                children: self.read_bookmarks(item.get(b"First").ok(), depth + 1, budget, seen),
            });
            next = item.get(b"Next").ok();
        }
        bookmarks
    }

    /// The page a destination leads to and how it is shown: an explicit
    /// `[page /XYZ …]` array, a dictionary holding one, or a name.
    fn destination(&self, dest: &Object, depth: u32) -> Option<(ObjectId, Vec<Object>)> {
        let array = match dest {
            Object::Name(name) | Object::String(name, _) => {
                self.named.get(name)?.as_array().ok()?
            }
            Object::Dictionary(d) if depth == 0 => {
                return self.destination(self.resolve(d.get(b"D").ok()?), 1);
            }
            Object::Array(array) => array,
            _ => return None,
        };
        let page = match array.first()? {
            Object::Reference(id) if self.page_index.contains_key(id) => *id,
            // Some writers give the position of the page instead.
            Object::Integer(i) => *self.pages.get(usize::try_from(*i).ok()?)?,
            _ => return None,
        };
        let view = array[1..].iter().map(|o| self.resolve(o).clone()).collect();
        Some((page, view))
    }

    fn read_form(&self) -> Option<Form> {
        let form = self.catalog_entry(b"AcroForm")?.as_dict().ok()?;
        if form.has(b"XFA") {
            return None;
        }
        let references = |key: &[u8]| -> Vec<ObjectId> {
            form.get(key)
                .map(|o| self.resolve(o))
                .and_then(Object::as_array)
                .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
                .unwrap_or_default()
        };
        let fields = references(b"Fields");
        if fields.is_empty() || self.signed(&fields) {
            return None;
        }
        Some(Form {
            fields,
            resources: form.get(b"DR").ok().cloned(),
            entries: [b"DA" as &[u8], b"Q", b"NeedAppearances"]
                .into_iter()
                .filter_map(|key| Some((key, self.resolve(form.get(key).ok()?).clone())))
                .collect(),
            order: references(b"CO"),
        })
    }

    /// Some field holds a digital signature.
    fn signed(&self, fields: &[ObjectId]) -> bool {
        let mut nodes: Vec<(ObjectId, u32)> = fields.iter().map(|&f| (f, 0)).collect();
        let mut visited = 0;
        while let Some((id, depth)) = nodes.pop() {
            visited += 1;
            if depth > MAX_DEPTH || visited > MAX_ITEMS {
                break;
            }
            let Ok(field) = self.doc.get_dictionary(id) else {
                continue;
            };
            if field
                .get(b"FT")
                .and_then(Object::as_name)
                .is_ok_and(|t| t == b"Sig")
                && field.has(b"V")
            {
                return true;
            }
            if let Ok(kids) = field
                .get(b"Kids")
                .map(|k| self.resolve(k))
                .and_then(Object::as_array)
            {
                nodes.extend(
                    kids.iter()
                        .filter_map(|k| k.as_reference().ok())
                        .map(|k| (k, depth + 1)),
                );
            }
        }
        false
    }
}

/// Follows references until a direct object.
fn resolve<'a>(doc: &'a Document, mut object: &'a Object) -> &'a Object {
    for _ in 0..32 {
        match object {
            Object::Reference(id) => match doc.get_object(*id) {
                Ok(next) => object = next,
                Err(_) => return &Object::Null,
            },
            _ => return object,
        }
    }
    &Object::Null
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
        if depth > MAX_DEPTH || visited > MAX_ITEMS {
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

/// A bookmark placed in a new document.
struct Placed {
    title: Object,
    link: Option<Link>,
    style: Vec<(&'static [u8], Object)>,
    open: bool,
    children: Vec<Placed>,
}

/// Where a bookmark leads.
#[derive(Clone)]
enum Link {
    /// A page of the document (`[page /XYZ …]`).
    Dest(Object),
    /// Another action.
    Action(Object),
}

/// The first page a list of bookmarks (or those under them) leads to.
fn first_dest(bookmarks: &[Placed]) -> Option<Link> {
    bookmarks.iter().find_map(|b| match &b.link {
        Some(link @ Link::Dest(_)) => Some(link.clone()),
        _ => first_dest(&b.children),
    })
}

/// A PDF being written, page by page.
struct Writer {
    out: Document,
    pages_id: ObjectId,
    kids: Vec<Object>,
    /// New number of each object copied (or about to be), by file and old number.
    copied: HashMap<(usize, ObjectId), ObjectId>,
    /// Pages of each file in this document.
    placed: HashSet<(usize, ObjectId)>,
    /// Annotations of those pages.
    annots: HashSet<(usize, ObjectId)>,
}

impl Writer {
    fn new() -> Self {
        let mut out = Document::with_version("1.7");
        let pages_id = out.new_object_id();
        Self {
            out,
            pages_id,
            kids: Vec::new(),
            copied: HashMap::new(),
            placed: HashSet::new(),
            annots: HashSet::new(),
        }
    }

    /// Adds `pages` of `inputs`, in order and turned, with what they use
    /// (contents, fonts, pictures, annotations): the other pages and the
    /// document structure stay behind. Links to pages left out are removed.
    fn add(&mut self, inputs: &[Input], pages: &[PdfPage]) -> Result<(), CoreError> {
        let missing = |page: &PdfPage| CoreError::InvalidPages {
            details: format!("page {} does not exist", page.number),
        };
        // Every page is numbered first, so that links between pages work
        // whatever their order.
        let mut olds = Vec::with_capacity(pages.len());
        for page in pages {
            let file = page.file as usize;
            let old = match inputs.get(file) {
                Some(Input::Pdf(source)) => {
                    let old = (page.number as usize)
                        .checked_sub(1)
                        .and_then(|i| source.pages.get(i))
                        .copied()
                        .ok_or_else(|| missing(page))?;
                    if !self.placed.insert((file, old)) {
                        return Err(CoreError::InvalidPages {
                            details: "a page is listed twice".into(),
                        });
                    }
                    self.copied.insert((file, old), self.out.new_object_id());
                    self.annots.extend(
                        source
                            .annots
                            .get(&old)
                            .into_iter()
                            .flatten()
                            .map(|&a| (file, a)),
                    );
                    Some(old)
                }
                Some(Input::Picture { .. }) if page.number == 1 => None,
                _ => return Err(missing(page)),
            };
            olds.push(old);
        }

        for (page, old) in pages.iter().zip(olds) {
            let file = page.file as usize;
            let id = match (&inputs[file], old) {
                (Input::Pdf(source), Some(old)) => {
                    self.import(file, source, &[old]);
                    self.copied[&(file, old)]
                }
                (
                    Input::Picture {
                        jpeg,
                        width,
                        height,
                        placement,
                    },
                    _,
                ) => self.add_picture(jpeg.clone(), *width, *height, *placement),
                (Input::Pdf(_), None) => continue,
            };
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

    /// A page holding a picture; gives its number.
    fn add_picture(
        &mut self,
        jpeg: Vec<u8>,
        width: u32,
        height: u32,
        placement: Placement,
    ) -> ObjectId {
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
            "MediaBox" => vec![0.into(), 0.into(), placement.page_width.into(), placement.page_height.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
            "Contents" => content_id,
        };
        self.out.add_object(page)
    }

    /// Copies the objects `roots` of file `file` with everything they use,
    /// unless already copied. Pages that are not in this document, their
    /// annotations and the objects of the document as a whole stay behind:
    /// references to them become null.
    fn import(&mut self, file: usize, source: &Source, roots: &[ObjectId]) {
        let doc = &source.doc;
        let mut queue = VecDeque::new();
        for &root in roots {
            match self.copied.get(&(file, root)) {
                // A page of this document is numbered before being copied.
                Some(new)
                    if self.placed.contains(&(file, root))
                        && !self.out.objects.contains_key(new) => {}
                Some(_) => continue,
                None if self.left_out(file, source, root) => continue,
                None => {
                    let new = self.out.new_object_id();
                    self.copied.insert((file, root), new);
                }
            }
            queue.push_back(root);
        }
        let mut fresh = Vec::new();
        let mut found = Vec::new();
        while let Some(id) = queue.pop_front() {
            fresh.push(id);
            let Ok(object) = doc.get_object(id) else {
                continue;
            };
            found.clear();
            references(object, self.placed.contains(&(file, id)), &mut found);
            if let Some(dest) = named_target(source, object) {
                references(dest, false, &mut found);
            }
            for &r in &found {
                if !self.copied.contains_key(&(file, r)) && !self.left_out(file, source, r) {
                    self.copied.insert((file, r), self.out.new_object_id());
                    queue.push_back(r);
                }
            }
        }

        for old in fresh {
            let new = self.copied[&(file, old)];
            let mut object = doc.get_object(old).cloned().unwrap_or(Object::Null);
            if self.placed.contains(&(file, old))
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
            renumber(&mut object, file, &self.copied);
            self.out.objects.insert(new, object);
        }
    }

    /// Objects of a file that never come along: pages that are not in this
    /// document, their annotations and the document structure.
    fn left_out(&self, file: usize, source: &Source, id: ObjectId) -> bool {
        (source.page_index.contains_key(&id) && !self.placed.contains(&(file, id)))
            || (source.all_annots.contains(&id) && !self.annots.contains(&(file, id)))
            || source.doc.get_object(id).map_or(true, document_level)
    }

    /// A copy of `object` of file `file` in this document, with what it uses.
    fn import_object(&mut self, file: usize, source: &Source, object: &Object) -> Object {
        let mut found = Vec::new();
        references(object, false, &mut found);
        self.import(file, source, &found);
        let mut object = object.clone();
        renumber(&mut object, file, &self.copied);
        object
    }

    /// Follows references among the objects written so far.
    fn resolved(&self, object: &Object) -> Object {
        resolve(&self.out, object).clone()
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
        // Annotations left behind with their pages are null now.
        if !list.iter().any(|a| dead(a) || matches!(a, Object::Null)) {
            return;
        }
        let alive: Vec<Object> = list
            .iter()
            .filter(|a| !dead(a) && !matches!(a, Object::Null))
            .cloned()
            .collect();
        if let Ok(dict) = self.out.get_dictionary_mut(page) {
            dict.set("Annots", alive);
        }
    }

    /// A bookmark in this document: it leads to its page if the page is
    /// here, otherwise to the first page of its section still here; it is
    /// gone if nothing of its section is left.
    fn place_bookmark(
        &mut self,
        file: usize,
        source: &Source,
        bookmark: &Bookmark,
    ) -> Option<Placed> {
        let children: Vec<Placed> = bookmark
            .children
            .iter()
            .filter_map(|b| self.place_bookmark(file, source, b))
            .collect();
        let link = match (&bookmark.target, &bookmark.action) {
            (Some((page, view)), _) => {
                if self.placed.contains(&(file, *page)) {
                    let mut dest = vec![Object::Reference(self.copied[&(file, *page)])];
                    dest.extend(view.iter().cloned());
                    Some(Link::Dest(Object::Array(dest)))
                } else if children.is_empty() {
                    return None;
                } else {
                    first_dest(&children)
                }
            }
            (None, Some(action)) => Some(Link::Action(self.import_object(file, source, action))),
            // A heading whose section is gone.
            (None, None) if !bookmark.children.is_empty() && children.is_empty() => return None,
            (None, None) => None,
        };
        Some(Placed {
            title: bookmark.title.clone(),
            link,
            style: bookmark.style.clone(),
            open: bookmark.open,
            children,
        })
    }

    /// Writes the bookmarks; gives the outline's number.
    fn write_outline(&mut self, bookmarks: Vec<Placed>) -> ObjectId {
        let root = self.out.new_object_id();
        let (first, last, visible) = self.write_bookmarks(bookmarks, root);
        self.out.objects.insert(
            root,
            Object::Dictionary(dictionary! {
                "Type" => "Outlines", "First" => first, "Last" => last, "Count" => visible,
            }),
        );
        root
    }

    /// Writes a list of bookmarks under `parent`: gives the first, the last
    /// and how many are shown (with those under the open ones).
    fn write_bookmarks(
        &mut self,
        bookmarks: Vec<Placed>,
        parent: ObjectId,
    ) -> (ObjectId, ObjectId, i64) {
        let ids: Vec<ObjectId> = bookmarks.iter().map(|_| self.out.new_object_id()).collect();
        let mut visible = 0;
        for (i, bookmark) in bookmarks.into_iter().enumerate() {
            let mut dict = dictionary! { "Title" => bookmark.title, "Parent" => parent };
            if i > 0 {
                dict.set("Prev", ids[i - 1]);
            }
            if let Some(&next) = ids.get(i + 1) {
                dict.set("Next", next);
            }
            if !bookmark.children.is_empty() {
                let (first, last, below) = self.write_bookmarks(bookmark.children, ids[i]);
                dict.set("First", first);
                dict.set("Last", last);
                dict.set("Count", if bookmark.open { below } else { -below });
                if bookmark.open {
                    visible += below;
                }
            }
            match bookmark.link {
                Some(Link::Dest(dest)) => dict.set("Dest", dest),
                Some(Link::Action(action)) => dict.set("A", action),
                None => {}
            }
            for (key, value) in bookmark.style {
                dict.set(key, value);
            }
            visible += 1;
            self.out.objects.insert(ids[i], Object::Dictionary(dict));
        }
        (ids[0], ids[ids.len() - 1], visible)
    }

    /// The fillable form of this document: the fields of each file with a
    /// widget on its pages here. Fields of different files with the same name
    /// would share their value: the later ones are numbered (`Nome_2`).
    fn write_form(&mut self, inputs: &[Input]) -> Option<ObjectId> {
        let mut fields: Vec<Object> = Vec::new();
        let mut taken: HashSet<Vec<u8>> = HashSet::new();
        let mut resources = Dictionary::new();
        let mut entries: Vec<(&[u8], Object)> = Vec::new();
        let mut order = Vec::new();
        for (file, input) in inputs.iter().enumerate() {
            let Input::Pdf(source) = input else { continue };
            let Some(form) = &source.form else { continue };
            let mut names = Vec::new();
            let before = fields.len();
            for &field in &form.fields {
                let Some(&new) = self.copied.get(&(file, field)) else {
                    continue;
                };
                if !self.prune_field(new, 0) {
                    continue;
                }
                if let Some(name) = self.field_name(new) {
                    let name = if taken.contains(&name) {
                        let renamed = (2..)
                            .map(|n| numbered(&name, n))
                            .find(|n| !taken.contains(n) && !names.contains(n))
                            .unwrap_or(name);
                        if let Ok(dict) = self.out.get_dictionary_mut(new) {
                            dict.set(
                                "T",
                                Object::String(renamed.clone(), lopdf::StringFormat::Literal),
                            );
                        }
                        renamed
                    } else {
                        name
                    };
                    names.push(name);
                }
                fields.push(new.into());
            }
            if fields.len() == before {
                continue;
            }
            taken.extend(names);
            order.extend(
                form.order
                    .iter()
                    .filter_map(|o| self.copied.get(&(file, *o)))
                    .map(|&n| Object::Reference(n)),
            );
            if let Some(dr) = &form.resources {
                let dr = self.import_object(file, source, dr);
                self.merge_resources(&mut resources, &dr);
            }
            for (key, value) in &form.entries {
                if !entries.iter().any(|(k, _)| k == key) {
                    entries.push((key, value.clone()));
                }
            }
        }
        if fields.is_empty() {
            return None;
        }
        let mut form = dictionary! { "Fields" => fields };
        if !resources.is_empty() {
            form.set("DR", resources);
        }
        for (key, value) in entries {
            form.set(key, value);
        }
        if !order.is_empty() {
            form.set("CO", order);
        }
        Some(self.out.add_object(form))
    }

    /// Removes the widgets left behind from a copied field (and from the
    /// fields under it): false when nothing of it is left.
    fn prune_field(&mut self, id: ObjectId, depth: u32) -> bool {
        let Ok(field) = self.out.get_dictionary(id) else {
            return false;
        };
        let widget = field
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_ok_and(|s| s == b"Widget");
        let Ok(kids) = field.get(b"Kids").cloned() else {
            return true;
        };
        if depth > MAX_DEPTH {
            return true;
        }
        let Object::Array(kids) = self.resolved(&kids) else {
            return widget;
        };
        let alive: Vec<Object> = kids
            .into_iter()
            .filter(|kid| match kid {
                Object::Reference(k) => self.prune_field(*k, depth + 1),
                _ => false,
            })
            .collect();
        let any = !alive.is_empty();
        if let Ok(field) = self.out.get_dictionary_mut(id) {
            field.set("Kids", alive);
        }
        any || widget
    }

    /// The partial name of a field (`T`).
    fn field_name(&self, id: ObjectId) -> Option<Vec<u8>> {
        let name = self.out.get_dictionary(id).ok()?.get(b"T").ok()?;
        match self.resolved(name) {
            Object::String(name, _) => Some(name),
            _ => None,
        }
    }

    /// Adds the field resources `dr` (fonts…) of another file to `merged`:
    /// names already there keep their resource.
    fn merge_resources(&self, merged: &mut Dictionary, dr: &Object) {
        let Object::Dictionary(dr) = self.resolved(dr) else {
            return;
        };
        for (category, value) in dr.iter() {
            let Object::Dictionary(value) = self.resolved(value) else {
                if !merged.has(category) {
                    merged.set(category.clone(), value.clone());
                }
                continue;
            };
            let mut entry = match merged.get(category) {
                Ok(Object::Dictionary(d)) => d.clone(),
                _ => Dictionary::new(),
            };
            for (name, resource) in value.iter() {
                if !entry.has(name) {
                    entry.set(name.clone(), resource.clone());
                }
            }
            merged.set(category.clone(), entry);
        }
    }

    fn finish(mut self, inputs: &[Input]) -> Result<Vec<u8>, CoreError> {
        let count = self.kids.len() as i64;
        let kids = std::mem::take(&mut self.kids);
        self.out.objects.insert(
            self.pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
        );
        let mut catalog = dictionary! { "Type" => "Catalog", "Pages" => self.pages_id };
        // The bookmarks of each file with pages here, one file after the other.
        let mut bookmarks = Vec::new();
        let mut show_outline = false;
        for (file, input) in inputs.iter().enumerate() {
            let Input::Pdf(source) = input else { continue };
            if !source
                .pages
                .iter()
                .any(|p| self.placed.contains(&(file, *p)))
            {
                continue;
            }
            for bookmark in &source.outline {
                if let Some(placed) = self.place_bookmark(file, source, bookmark) {
                    bookmarks.push(placed);
                }
            }
            show_outline |= source.show_outline;
        }
        if !bookmarks.is_empty() {
            let outline = self.write_outline(bookmarks);
            catalog.set("Outlines", outline);
            if show_outline {
                catalog.set("PageMode", "UseOutlines");
            }
        }
        if let Some(form) = self.write_form(inputs) {
            catalog.set("AcroForm", form);
        }
        let catalog_id = self.out.add_object(catalog);
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

/// `name` followed by `_n`, in the encoding of the name (PDFDocEncoding or
/// UTF-16 with a byte order mark).
fn numbered(name: &[u8], n: u32) -> Vec<u8> {
    let suffix = format!("_{n}");
    let mut numbered = name.to_vec();
    if name.starts_with(&[0xFE, 0xFF]) {
        numbered.extend(suffix.encode_utf16().flat_map(u16::to_be_bytes));
    } else {
        numbered.extend(suffix.bytes());
    }
    numbered
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

/// Gives the references of an object of file `file` their new numbers; those
/// to objects left behind become null.
fn renumber(object: &mut Object, file: usize, numbers: &HashMap<(usize, ObjectId), ObjectId>) {
    match object {
        Object::Reference(id) => {
            let new = numbers
                .get(&(file, *id))
                .map_or(Object::Null, |n| Object::Reference(*n));
            *object = new;
        }
        Object::Array(items) => items.iter_mut().for_each(|o| renumber(o, file, numbers)),
        Object::Dictionary(dict) => dict
            .iter_mut()
            .for_each(|(_, o)| renumber(o, file, numbers)),
        Object::Stream(stream) => stream
            .dict
            .iter_mut()
            .for_each(|(_, o)| renumber(o, file, numbers)),
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
        let (merged, pages) =
            assemble(vec![Part::Document(first), Part::Document(second)]).unwrap();
        assert_eq!(pages, 3);
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
        let (pdf, pages) = assemble(vec![
            Part::Picture {
                jpeg: vec![0xff, 0xd8, 0xff, 0xd9],
                width: 400,
                height: 300,
                placement,
            },
            Part::Document(sample_pdf(&[(A4.0, A4.1)])),
        ])
        .unwrap();
        assert_eq!(pages, 2);
        let doc = Document::load_mem(&pdf).unwrap();
        assert_eq!(doc.get_pages().len(), 2);
        assert!(is_pdf_bytes(&pdf));
    }

    fn is_pdf_bytes(data: &[u8]) -> bool {
        data.starts_with(b"%PDF-")
    }

    fn page(number: u32, rotation: u16) -> PdfPage {
        PdfPage {
            file: 0,
            number,
            rotation,
        }
    }

    fn page_of(file: u32, number: u32, rotation: u16) -> PdfPage {
        PdfPage {
            file,
            number,
            rotation,
        }
    }

    /// Edits `data` into a single document.
    fn edit_one(data: &[u8], pages: &[PdfPage]) -> Vec<u8> {
        let mut result = Vec::new();
        edit(
            data.to_vec(),
            pages,
            &[],
            &AtomicBool::new(false),
            |_, bytes| {
                result.push(bytes);
                true
            },
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        result.pop().unwrap()
    }

    /// Builds the pages of `layout` into a single document.
    fn build_one(files: Vec<Part>, layout: &[PdfPage]) -> Vec<u8> {
        build_parts(files, layout, &[]).pop().unwrap()
    }

    /// Builds the pages of `layout` into documents of `parts` pages.
    fn build_parts(files: Vec<Part>, layout: &[PdfPage], parts: &[u32]) -> Vec<Vec<u8>> {
        let mut result = Vec::new();
        build(
            files,
            Some(layout),
            parts,
            &AtomicBool::new(false),
            |_, bytes| {
                result.push(bytes);
                true
            },
        )
        .unwrap();
        result
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
        let layout = [
            page(2, 180),
            page_of(1, 1, 0),
            page_of(1, 2, 0),
            page_of(1, 3, 0),
        ];
        let merged = build_one(
            vec![Part::Document(data.clone()), Part::Document(data)],
            &layout,
        );
        assert_eq!(page_count(&merged).unwrap(), 4);
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
        assert_eq!(bookmark_pages(&open(data.clone()).unwrap()), [1]);
        assert_eq!(Source::open(&data).unwrap().outline.len(), 1);

        // All pages: every link still works, the named one now points directly.
        let all = edit_one(&data, &[page(1, 0), page(2, 0), page(3, 0)]);
        assert_eq!(link_targets(&all), [Some(1), Some(2), Some(2)]);
        // Page 2 is gone: so is the link to it.
        let without_second = edit_one(&data, &[page(1, 0), page(3, 0)]);
        assert_eq!(link_targets(&without_second), [Some(1), Some(1)]);
        // Page 3 is gone: only the link to page 2 is left.
        let without_third = edit_one(&data, &[page(1, 0), page(2, 0)]);
        assert_eq!(link_targets(&without_third), [Some(1)]);

        // The bookmark to page 1 stays; names and page dates are not carried
        // over; the font is shared.
        let doc = Document::load_mem(&without_second).unwrap();
        let catalog = doc.catalog().unwrap();
        assert!(catalog.has(b"Outlines") && !catalog.has(b"Names"));
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
            data.clone(),
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
        let err = edit(
            data.clone(),
            &pages,
            &[2, 2],
            &AtomicBool::new(false),
            |_, _| true,
        );
        assert_eq!(err.unwrap_err().code(), "invalid_pages");
        // Stopped after the first part, or before any.
        let err = edit(
            data.clone(),
            &pages,
            &[2, 3],
            &AtomicBool::new(false),
            |_, _| false,
        );
        assert_eq!(err.unwrap_err().code(), "cancelled");
        let err = edit(
            data.clone(),
            &pages,
            &[2, 3],
            &AtomicBool::new(true),
            |_, _| true,
        );
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

    /// Four A4 pages with bookmarks and a fillable form:
    /// * "Capitolo 1" (page 1, open) with "1.1" (page 2) and "1.2" (page 3,
    ///   through a `GoTo` action); "Capitolo 2" (page 4, by name); "Sito" (a
    ///   mail link);
    /// * fields "Nome" (widget on page 1), "Firma" (widgets on pages 2 and
    ///   3) and "Data" (page 4), with a default font.
    ///
    /// With `xfa`, the form is an XFA form.
    fn structured_pdf(xfa: bool) -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let ids: Vec<ObjectId> = (0..4).map(|_| doc.new_object_id()).collect();
        let rect = || -> Vec<Object> { vec![72.into(), 72.into(), 272.into(), 100.into()] };
        let widget = |doc: &mut Document, page: ObjectId, extra: Dictionary| {
            let mut dict = dictionary! {
                "Type" => "Annot", "Subtype" => "Widget", "Rect" => rect(), "P" => page,
            };
            dict.extend(&extra);
            doc.add_object(dict)
        };
        let nome = widget(
            &mut doc,
            ids[0],
            dictionary! {
                "FT" => "Tx", "T" => Object::string_literal("Nome"), "V" => Object::string_literal("Ada"),
            },
        );
        let firma = doc.new_object_id();
        let firma_kids: Vec<ObjectId> = [ids[1], ids[2]]
            .iter()
            .map(|&p| widget(&mut doc, p, dictionary! { "Parent" => firma }))
            .collect();
        doc.objects.insert(
            firma,
            Object::Dictionary(dictionary! {
                "FT" => "Tx", "T" => Object::string_literal("Firma"),
                "Kids" => firma_kids.iter().map(|&k| Object::from(k)).collect::<Vec<_>>(),
            }),
        );
        let data = widget(
            &mut doc,
            ids[3],
            dictionary! { "FT" => "Tx", "T" => Object::string_literal("Data") },
        );
        let annots = [
            vec![nome],
            vec![firma_kids[0]],
            vec![firma_kids[1]],
            vec![data],
        ];
        for (i, &id) in ids.iter().enumerate() {
            let text = format!("BT /F1 24 Tf 72 720 Td (Pagina {}) Tj ET", i + 1);
            let content = doc.add_object(Stream::new(Dictionary::new(), text.into_bytes()));
            let annots: Vec<Object> = annots[i].iter().map(|&a| a.into()).collect();
            doc.objects.insert(
                id,
                Object::Dictionary(dictionary! {
                    "Type" => "Page", "Parent" => pages_id,
                    "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                    "Contents" => content,
                    "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
                    "Annots" => annots,
                }),
            );
        }
        let kids: Vec<Object> = ids.iter().map(|id| (*id).into()).collect();
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 4 }),
        );

        let outlines = doc.new_object_id();
        let fit = |page: ObjectId| -> Object { vec![page.into(), "Fit".into()].into() };
        let [chapter1, one, two, chapter2, site] = [(); 5].map(|_| doc.new_object_id());
        let item = |title: &str, parent: ObjectId| {
            dictionary! { "Title" => Object::string_literal(title), "Parent" => parent }
        };
        let mut c1 = item("Capitolo 1", outlines);
        c1.extend(&dictionary! {
            "Dest" => fit(ids[0]), "Next" => chapter2, "First" => one, "Last" => two, "Count" => 2,
            "C" => vec![1.into(), 0.into(), 0.into()],
        });
        let mut i1 = item("1.1", chapter1);
        i1.extend(&dictionary! { "Dest" => fit(ids[1]), "Next" => two });
        let mut i2 = item("1.2", chapter1);
        i2.extend(&dictionary! {
            "A" => dictionary! { "S" => "GoTo", "D" => fit(ids[2]) }, "Prev" => one,
        });
        let mut c2 = item("Capitolo 2", outlines);
        c2.extend(&dictionary! {
            "Dest" => Object::string_literal("cap2"), "Prev" => chapter1, "Next" => site,
        });
        let mut web = item("Sito", outlines);
        web.extend(&dictionary! {
            "A" => dictionary! { "S" => "URI", "URI" => Object::string_literal("mailto:posta@example.invalid") },
            "Prev" => chapter2,
        });
        for (id, dict) in [
            (chapter1, c1),
            (one, i1),
            (two, i2),
            (chapter2, c2),
            (site, web),
        ] {
            doc.objects.insert(id, Object::Dictionary(dict));
        }
        doc.objects.insert(
            outlines,
            Object::Dictionary(dictionary! {
                "Type" => "Outlines", "First" => chapter1, "Last" => site, "Count" => 5,
            }),
        );
        let mut form = dictionary! {
            "Fields" => vec![nome.into(), firma.into(), data.into()],
            "DR" => dictionary! { "Font" => dictionary! { "Helv" => font } },
            "DA" => Object::string_literal("/Helv 0 Tf 0 g"),
        };
        if xfa {
            form.set("XFA", Object::string_literal("<xdp/>"));
        }
        let names = dictionary! {
            "Dests" => dictionary! {
                "Names" => vec![Object::string_literal("cap2"), fit(ids[3])],
            },
        };
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => pages_id, "Outlines" => outlines,
            "Names" => names, "AcroForm" => form, "PageMode" => "UseOutlines",
        });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    /// One bookmark as the tests see it: depth, title, the position of the
    /// page it leads to, and its `Count`.
    type Seen = (u32, String, Option<usize>, i64);

    fn bookmark(depth: u32, title: &str, page: Option<usize>, count: i64) -> Seen {
        (depth, title.to_string(), page, count)
    }

    /// The bookmarks of a document, in order.
    fn outline_of(data: &[u8]) -> Vec<Seen> {
        fn walk(
            doc: &Document,
            pages: &[ObjectId],
            first: Option<ObjectId>,
            depth: u32,
            found: &mut Vec<Seen>,
        ) {
            let mut next = first;
            while let Some(id) = next {
                let item = doc.get_dictionary(id).unwrap();
                let title = String::from_utf8_lossy(item.get(b"Title").unwrap().as_str().unwrap());
                let page = item
                    .get(b"Dest")
                    .ok()
                    .and_then(|d| d.as_array().ok()?.first()?.as_reference().ok())
                    .and_then(|p| pages.iter().position(|q| *q == p));
                let count = item.get(b"Count").and_then(Object::as_i64).unwrap_or(0);
                found.push((depth, title.into_owned(), page, count));
                let first = item.get(b"First").and_then(Object::as_reference).ok();
                walk(doc, pages, first, depth + 1, found);
                next = item.get(b"Next").and_then(Object::as_reference).ok();
            }
        }
        let doc = Document::load_mem(data).unwrap();
        let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
        let mut found = Vec::new();
        let Ok(outlines) = doc
            .catalog()
            .unwrap()
            .get(b"Outlines")
            .and_then(Object::as_reference)
        else {
            return found;
        };
        let first = doc
            .get_dictionary(outlines)
            .unwrap()
            .get(b"First")
            .and_then(Object::as_reference)
            .ok();
        walk(&doc, &pages, first, 0, &mut found);
        found
    }

    /// Names of the top fields of a document's form, each with the number of
    /// its widgets.
    fn fields_of(data: &[u8]) -> Vec<(String, usize)> {
        let doc = Document::load_mem(data).unwrap();
        let Ok(form) = doc.catalog().unwrap().get(b"AcroForm") else {
            return Vec::new();
        };
        let form = match form {
            Object::Reference(id) => doc.get_dictionary(*id).unwrap(),
            other => other.as_dict().unwrap(),
        };
        form.get(b"Fields")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                let field = doc.get_dictionary(f.as_reference().unwrap()).unwrap();
                let name = String::from_utf8_lossy(field.get(b"T").unwrap().as_str().unwrap());
                let widgets = field
                    .get(b"Kids")
                    .and_then(Object::as_array)
                    .map_or(1, Vec::len);
                (name.into_owned(), widgets)
            })
            .collect()
    }

    fn field(name: &str, widgets: usize) -> (String, usize) {
        (name.to_string(), widgets)
    }

    #[test]
    fn bookmarks_and_forms_are_found_by_the_analysis() {
        let pdf = open(structured_pdf(false)).unwrap();
        // "Sito" leads out of the document; "Capitolo 2" goes by its name.
        assert_eq!(bookmark_pages(&pdf), [1, 2, 3, 4]);
        assert_eq!(form_kind(&pdf), PdfForm::Fillable);
        assert_eq!(
            form_kind(&open(structured_pdf(true)).unwrap()),
            PdfForm::Fixed
        );
        let plain = open(sample_pdf(&[(A4.0, A4.1)])).unwrap();
        assert!(bookmark_pages(&plain).is_empty());
        assert_eq!(form_kind(&plain), PdfForm::None);
    }

    #[test]
    fn bookmarks_follow_their_pages() {
        let data = structured_pdf(false);
        let all = [page(1, 0), page(2, 0), page(3, 0), page(4, 0)];
        // Every page: the same bookmarks, with named and action destinations
        // made explicit, the first chapter open, the mail link kept.
        let same = edit_one(&data, &all);
        assert_eq!(
            outline_of(&same),
            [
                bookmark(0, "Capitolo 1", Some(0), 2),
                bookmark(1, "1.1", Some(1), 0),
                bookmark(1, "1.2", Some(2), 0),
                bookmark(0, "Capitolo 2", Some(3), 0),
                bookmark(0, "Sito", None, 0),
            ]
        );
        let doc = Document::load_mem(&same).unwrap();
        let catalog = doc.catalog().unwrap();
        assert_eq!(
            catalog.get(b"PageMode").unwrap().as_name().unwrap(),
            b"UseOutlines"
        );
        let outlines = catalog
            .get(b"Outlines")
            .and_then(Object::as_reference)
            .unwrap();
        let count = doc.get_dictionary(outlines).unwrap().get(b"Count");
        assert_eq!(count.unwrap().as_i64().unwrap(), 5);
        assert!(String::from_utf8_lossy(&same).contains("mailto:posta@example.invalid"));

        // Reordered: the bookmarks keep their order and follow their pages.
        let reordered = edit_one(&data, &[page(4, 0), page(3, 90), page(2, 0), page(1, 0)]);
        let targets: Vec<Option<usize>> = outline_of(&reordered).iter().map(|b| b.2).collect();
        assert_eq!(targets, [Some(3), Some(2), Some(1), Some(0), None]);

        // The first page is gone: its chapter leads to its first section left.
        let without_first = edit_one(&data, &[page(2, 0), page(3, 0), page(4, 0)]);
        assert_eq!(
            outline_of(&without_first)[0],
            bookmark(0, "Capitolo 1", Some(0), 2)
        );
        // Its sections are gone: the chapter is left on its own.
        let without_sections = edit_one(&data, &[page(1, 0), page(4, 0)]);
        assert_eq!(
            outline_of(&without_sections),
            [
                bookmark(0, "Capitolo 1", Some(0), 0),
                bookmark(0, "Capitolo 2", Some(1), 0),
                bookmark(0, "Sito", None, 0),
            ]
        );
        // The whole chapter is gone.
        let last = edit_one(&data, &[page(4, 0)]);
        assert_eq!(
            outline_of(&last),
            [
                bookmark(0, "Capitolo 2", Some(0), 0),
                bookmark(0, "Sito", None, 0)
            ]
        );

        // Split in two: each document has the bookmarks of its pages.
        let parts = build_parts(vec![Part::Document(data.clone())], &all, &[2, 2]);
        assert_eq!(
            outline_of(&parts[0]),
            [
                bookmark(0, "Capitolo 1", Some(0), 1),
                bookmark(1, "1.1", Some(1), 0),
                bookmark(0, "Sito", None, 0),
            ]
        );
        assert_eq!(
            outline_of(&parts[1]),
            [
                bookmark(0, "Capitolo 1", Some(0), 1),
                bookmark(1, "1.2", Some(0), 0),
                bookmark(0, "Capitolo 2", Some(1), 0),
                bookmark(0, "Sito", None, 0),
            ]
        );
    }

    #[test]
    fn forms_keep_the_fields_of_their_pages() {
        let data = structured_pdf(false);
        let all = [page(1, 0), page(2, 0), page(3, 0), page(4, 0)];
        let same = edit_one(&data, &all);
        assert_eq!(
            fields_of(&same),
            [field("Nome", 1), field("Firma", 2), field("Data", 1)]
        );
        let doc = Document::load_mem(&same).unwrap();
        let form = doc
            .catalog()
            .unwrap()
            .get(b"AcroForm")
            .and_then(Object::as_reference)
            .unwrap();
        let form = doc.get_dictionary(form).unwrap();
        assert_eq!(
            form.get(b"DA").unwrap().as_str().unwrap(),
            b"/Helv 0 Tf 0 g"
        );
        assert!(form.get(b"DR").unwrap().as_dict().unwrap().has(b"Font"));
        // The value of a field comes along.
        assert!(String::from_utf8_lossy(&same).contains("(Ada)"));

        // Page 3 is gone, and one widget of "Firma" with it.
        let without_third = edit_one(&data, &[page(1, 0), page(2, 0), page(4, 0)]);
        assert_eq!(
            fields_of(&without_third),
            [field("Nome", 1), field("Firma", 1), field("Data", 1)]
        );
        // Pages 2 and 3 are gone, and "Firma" with them.
        let without_signature = edit_one(&data, &[page(4, 0), page(1, 0)]);
        assert_eq!(
            fields_of(&without_signature),
            [field("Nome", 1), field("Data", 1)]
        );
        let doc = Document::load_mem(&without_signature).unwrap();
        let widgets = doc
            .objects
            .values()
            .filter(|o| {
                o.as_dict().is_ok_and(|d| {
                    d.get(b"Subtype")
                        .and_then(Object::as_name)
                        .is_ok_and(|s| s == b"Widget")
                })
            })
            .count();
        assert_eq!(widgets, 2);

        // Two copies of the same form: the fields of the second one are
        // renamed, so that each keeps its own value; "Firma" of the first
        // copy is not here, so the second one keeps its name.
        let twice = build_one(
            vec![Part::Document(data.clone()), Part::Document(data.clone())],
            &[page(1, 0), page_of(1, 1, 0), page_of(1, 2, 0)],
        );
        assert_eq!(
            fields_of(&twice),
            [field("Nome", 1), field("Nome_2", 1), field("Firma", 1)]
        );

        // An XFA form is not kept as a form: its widgets keep their look.
        let xfa = edit_one(&structured_pdf(true), &all);
        assert!(fields_of(&xfa).is_empty());
        assert_eq!(outline_of(&xfa).len(), 5);
    }

    #[test]
    fn pages_of_several_files_are_combined_in_any_order() {
        let first = linked_pdf();
        let second = sample_pdf(&[(200.0, 100.0), (300.0, 300.0)]);
        let photo = place(400, 300, &PdfOptions::default());
        let files = || {
            vec![
                Part::Document(first.clone()),
                Part::Document(second.clone()),
                Part::Picture {
                    jpeg: vec![0xff, 0xd8, 0xff, 0xd9],
                    width: 400,
                    height: 300,
                    placement: photo,
                },
            ]
        };
        let layout = [
            page_of(1, 2, 0),
            page(3, 0),
            page_of(2, 1, 90),
            page(1, 0),
            page_of(1, 1, 0),
        ];
        let combined = build_one(files(), &layout);
        let sizes: Vec<(f32, f32)> = open(combined.clone())
            .unwrap()
            .pages()
            .iter()
            .map(|p| p.render_dimensions())
            .collect();
        assert_eq!(
            sizes,
            [
                (300.0, 300.0),
                (595.0, 842.0),
                (photo.page_height, photo.page_width),
                (595.0, 842.0),
                (200.0, 100.0),
            ]
        );
        assert_eq!(rotations(&combined), [0, 0, 90, 0, 0]);
        // Page 1 of the first file (here the fourth) links twice to its page
        // 3 (here the second); its page 2 is not here, nor the link to it.
        let doc = Document::load_mem(&combined).unwrap();
        let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
        let annots = doc.get_dictionary(pages[3]).unwrap().get(b"Annots");
        assert_eq!(annots.unwrap().as_array().unwrap().len(), 2);
        // Its bookmark leads to its page 1, here the fourth page.
        assert_eq!(outline_of(&combined), [bookmark(0, "Inizio", Some(3), 0)]);
        // The shared font is copied once.
        let fonts = doc
            .objects
            .values()
            .filter(|o| o.as_dict().is_ok_and(|d| d.has_type(b"Font")))
            .count();
        assert_eq!(fonts, 1);

        // The combined pages, split in two documents.
        let parts = build_parts(files(), &layout, &[3, 2]);
        let counts: Vec<u32> = parts.iter().map(|p| page_count(p).unwrap()).collect();
        assert_eq!(counts, [3, 2]);
        // A picture listed twice, a page or a file that is not there.
        for wrong in [
            vec![page_of(2, 1, 0), page_of(2, 1, 0)],
            vec![page_of(2, 2, 0)],
            vec![page_of(3, 1, 0)],
        ] {
            let err = build(
                files(),
                Some(&wrong),
                &[],
                &AtomicBool::new(false),
                |_, _| true,
            );
            assert_eq!(err.unwrap_err().code(), "invalid_pages", "{wrong:?}");
        }
    }

    #[test]
    fn layouts_of_several_files_are_checked() {
        assert_eq!(
            check_pages(None, &[2, 1]).unwrap(),
            [page(1, 0), page(2, 0), page_of(1, 1, 0)]
        );
        assert_eq!(
            pages_of_file(&[page(2, 0), page_of(1, 1, 90), page(1, 0)], 1),
            [page(1, 90)]
        );
        assert!(check_pages(Some(&[page_of(1, 1, 0), page(1, 0)]), &[1, 1]).is_ok());
        let err = check_layout(Some(&[page_of(1, 1, 0)]), 3).unwrap_err();
        assert_eq!(err.code(), "invalid_pages");
    }
}
