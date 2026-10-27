//! Smaller PDFs ("Comprimi"), without FFmpeg:
//!
//! 1. hayro interprets every page with a device that only records where each
//!    picture lands, so Konvert knows the resolution it really has on paper.
//! 2. Pictures sharper than the level needs are scaled down and stored again:
//!    photos as JPEG, pictures with few colors as indexed Deflate (lossless),
//!    gray pictures without color channels. A new picture replaces the old
//!    one only when it is smaller.
//! 3. What the document does not use, metadata, page thumbnails and duplicate
//!    streams go; embedded TrueType fonts keep only the glyphs the document
//!    shows (see [`crate::pdf_fonts`]); streams are recompressed with the strongest Deflate setting
//!    and small objects are packed into object streams.
//! 4. Only if the user allows it, pages become JPEG pictures when that is
//!    smaller (their text can no longer be selected).
//!
//! Text, drawings, links, bookmarks and forms stay as they are (except on
//! pages made into pictures). With a size limit, stronger settings follow one
//! another until the document fits; each picture is decoded once for all of
//! them.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageData, ImageDrawProps,
    InterpreterCache, InterpreterSettings, LumaData, SoftMask, TransformExt, interpret_page,
};
use hayro::hayro_syntax::object::ObjectIdentifier;
use hayro::{RenderCache, kurbo};
use lopdf::{Dictionary, Document, Object, ObjectId, SaveOptions, Stream, dictionary};

use crate::error::CoreError;
use crate::pdf;
use crate::request::{CompressLevel, CompressOptions};

/// How pictures are stored again in one attempt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shrink {
    /// Pictures sharper than this many pixels per inch on paper are scaled down.
    pub dpi: f32,
    /// JPEG quality of photos (1–100).
    pub quality: u8,
    /// Pictures that are not photos (screenshots, charts) may keep only the
    /// 256 colors that represent them best.
    pub fewer_colors: bool,
}

/// Settings from the lightest to the strongest: a level starts at its own and
/// a size limit moves on to the next ones.
const STEPS: [Shrink; 7] = [
    Shrink {
        dpi: 200.0,
        quality: 82,
        fewer_colors: false,
    },
    Shrink {
        dpi: 144.0,
        quality: 68,
        fewer_colors: true,
    },
    Shrink {
        dpi: 96.0,
        quality: 50,
        fewer_colors: true,
    },
    Shrink {
        dpi: 72.0,
        quality: 40,
        fewer_colors: true,
    },
    Shrink {
        dpi: 60.0,
        quality: 32,
        fewer_colors: true,
    },
    Shrink {
        dpi: 50.0,
        quality: 25,
        fewer_colors: true,
    },
    Shrink {
        dpi: 40.0,
        quality: 20,
        fewer_colors: true,
    },
];

/// Pages drawn as pictures, from the lightest to the strongest.
const PAGE_STEPS: [Shrink; 6] = [
    Shrink {
        dpi: 150.0,
        quality: 70,
        fewer_colors: false,
    },
    Shrink {
        dpi: 120.0,
        quality: 60,
        fewer_colors: true,
    },
    Shrink {
        dpi: 96.0,
        quality: 50,
        fewer_colors: true,
    },
    Shrink {
        dpi: 72.0,
        quality: 40,
        fewer_colors: true,
    },
    Shrink {
        dpi: 60.0,
        quality: 32,
        fewer_colors: true,
    },
    Shrink {
        dpi: 50.0,
        quality: 25,
        fewer_colors: true,
    },
];

/// The steps of a level: its own, then the stronger ones when there is a size limit.
pub(crate) fn steps(level: CompressLevel, limited: bool) -> Vec<Shrink> {
    let first = first_step(level);
    if limited {
        STEPS[first..].to_vec()
    } else {
        vec![STEPS[first]]
    }
}

fn first_step(level: CompressLevel) -> usize {
    match level {
        CompressLevel::Light => 0,
        CompressLevel::Strong => 1,
        CompressLevel::Max => 2,
    }
}

/// A picture is scaled down only when it is noticeably sharper than needed.
const RESAMPLE_MARGIN: f32 = 1.25;
/// A new picture must save at least this fraction of the old one.
const MIN_GAIN: f64 = 0.03;
/// Below this many unique colors a picture is a graphic: kept pixel for pixel.
const GRAPHIC_COLORS: usize = 256;
/// Gray pictures with at most this many levels are graphics too (scans of text).
const GRAPHIC_GRAYS: usize = 16;

/// Compresses a PDF: the smallest document found for the options (it may be
/// larger than the limit when nothing smaller could be made). `progress` gets
/// the fraction done. Blocking: run it on a worker thread.
pub fn compress(
    data: &[u8],
    options: &CompressOptions,
    cancel: &AtomicBool,
    mut progress: impl FnMut(f64),
) -> Result<Vec<u8>, CoreError> {
    let fits = |size: usize| options.max_bytes.is_none_or(|max| size as u64 <= max);
    let first = first_step(options.level);
    let steps = steps(options.level, options.max_bytes.is_some());
    let pages_steps: Vec<Shrink> = match (options.pages_as_pictures, options.max_bytes) {
        (false, _) => Vec::new(),
        (true, Some(_)) => PAGE_STEPS[first..].to_vec(),
        (true, None) => vec![PAGE_STEPS[first]],
    };
    // Shares of the work: reading the pictures, writing each attempt, pages.
    let total = 2.0 + steps.len() as f64 + pages_steps.len() as f64;

    let base = Base::open(data)?;
    let pictures = guarded(|| survey(data, cancel))?;
    progress(1.0 / total);
    let recoded = guarded(|| {
        recode(data, &base, &pictures, &steps, cancel, |f| {
            progress((1.0 + f) / total)
        })
    })?;
    let mut best: Option<Vec<u8>> = None;
    for (i, _) in steps.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        let bytes = base.write(&recoded, i)?;
        progress((3.0 + i as f64) / total);
        let done = fits(bytes.len());
        if best.as_ref().is_none_or(|b| bytes.len() < b.len()) {
            best = Some(bytes);
        }
        if done {
            break;
        }
    }
    let mut best = best.expect("at least one attempt");
    if !fits(best.len()) || options.max_bytes.is_none() {
        for (i, step) in pages_steps.iter().enumerate() {
            let bytes = guarded(|| pages_as_pictures(data, *step, cancel))?;
            progress((3.0 + (steps.len() + i) as f64) / total);
            let done = fits(bytes.len());
            if bytes.len() < best.len() {
                best = bytes;
            }
            if done {
                break;
            }
        }
    }
    progress(1.0);
    Ok(best)
}

/// Runs hayro code: a renderer bug on one strange file must not take the app
/// down.
fn guarded<T>(f: impl FnOnce() -> Result<T, CoreError>) -> Result<T, CoreError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| {
        Err(CoreError::CorruptInput {
            details: "PDF: the reader failed".into(),
        })
    })
}

// --- Where pictures land ---------------------------------------------------------------

/// Largest size of a picture on paper, in points, over all its uses.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Placed {
    width: f32,
    height: f32,
}

fn object_id(id: ObjectIdentifier) -> Option<ObjectId> {
    let number = u32::try_from(id.obj_number).ok().filter(|n| *n > 0)?;
    Some((number, u16::try_from(id.gen_number).ok()?))
}

/// Size on paper of a `width`×`height` picture drawn with `transform` (which
/// maps its pixels to points).
fn placed_size(transform: kurbo::Affine, width: u32, height: u32) -> Placed {
    let [a, b, c, d, _, _] = transform.as_coeffs();
    Placed {
        width: (a.hypot(b) * width as f64) as f32,
        height: (c.hypot(d) * height as f64) as f32,
    }
}

/// Interprets every page and notes the largest size of each picture object.
fn survey(data: &[u8], cancel: &AtomicBool) -> Result<HashMap<ObjectId, Placed>, CoreError> {
    let pdf = pdf::open(data.to_vec())?;
    let mut device = Survey::default();
    let cache = InterpreterCache::new();
    for page in pdf.pages().iter() {
        if cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        let (width, height) = page.render_dimensions();
        let mut context = Context::new(
            page.initial_transform(true).to_kurbo(),
            kurbo::Rect::new(0.0, 0.0, width as f64, height as f64),
            &cache,
            page.xref(),
            InterpreterSettings::default(),
        );
        interpret_page(page, &mut context, &mut device);
    }
    Ok(device.placed)
}

#[derive(Default)]
struct Survey {
    placed: HashMap<ObjectId, Placed>,
}

impl<'a> Device<'a> for Survey {
    fn draw_path(&mut self, _: &kurbo::BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_glyph_run(&mut self, _: &GlyphRun<'_, 'a>, _: DrawProps<'a>, _: &DrawMode) {}
    fn draw_image(&mut self, image: Image<'a, '_>, props: ImageDrawProps<'a>) {
        let Image::Raster(raster) = image else {
            return;
        };
        let Some(id) = object_id(raster.stream().obj_id()) else {
            return;
        };
        let size = placed_size(props.transform, raster.width(), raster.height());
        let entry = self.placed.entry(id).or_default();
        entry.width = entry.width.max(size.width);
        entry.height = entry.height.max(size.height);
    }
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

// --- New pictures --------------------------------------------------------------------

/// A picture stored again: its new stream, and its new transparency.
#[derive(Debug, Clone)]
struct Recoded {
    image: Stream,
    mask: Option<Stream>,
}

impl Recoded {
    fn len(&self) -> usize {
        self.image.content.len() + self.mask.as_ref().map_or(0, |m| m.content.len())
    }
}

/// New versions of the pictures, for each step (`None`: keep the original).
type Versions = HashMap<ObjectId, Vec<Option<Recoded>>>;

/// Interprets the pages again, decoding each picture worth working on once,
/// and stores it again for every step.
fn recode(
    data: &[u8],
    base: &Base,
    placed: &HashMap<ObjectId, Placed>,
    steps: &[Shrink],
    cancel: &AtomicBool,
    mut progress: impl FnMut(f64),
) -> Result<Versions, CoreError> {
    let wanted: HashMap<ObjectId, Candidate> = placed
        .iter()
        .filter_map(|(id, place)| base.candidate(*id, *place).map(|c| (*id, c)))
        .collect();
    let pdf = pdf::open(data.to_vec())?;
    let interpreter_cache = InterpreterCache::new();
    let mut device = Recoder {
        wanted,
        steps,
        done: HashMap::new(),
    };
    let pages = pdf.pages();
    for (i, page) in pages.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        if device.wanted.is_empty() {
            break;
        }
        let (width, height) = page.render_dimensions();
        let mut context = Context::new(
            page.initial_transform(true).to_kurbo(),
            kurbo::Rect::new(0.0, 0.0, width as f64, height as f64),
            &interpreter_cache,
            page.xref(),
            InterpreterSettings::default(),
        );
        interpret_page(page, &mut context, &mut device);
        progress((i + 1) as f64 / pages.len().max(1) as f64);
    }
    Ok(device.done)
}

/// What Konvert knows of a picture before decoding it.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    place: Placed,
    /// Bytes of the stored picture and of its transparency.
    stored: usize,
    /// Stored as JPEG or JPEG 2000 (lossy: colors are not exact).
    lossy: bool,
}

struct Recoder<'s> {
    wanted: HashMap<ObjectId, Candidate>,
    steps: &'s [Shrink],
    done: Versions,
}

impl<'a> Device<'a> for Recoder<'_> {
    fn draw_path(&mut self, _: &kurbo::BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_glyph_run(&mut self, _: &GlyphRun<'_, 'a>, _: DrawProps<'a>, _: &DrawMode) {}
    fn draw_image(&mut self, image: Image<'a, '_>, _: ImageDrawProps<'a>) {
        let Image::Raster(raster) = image else {
            return;
        };
        let Some(id) = object_id(raster.stream().obj_id()) else {
            return;
        };
        let Some(candidate) = self.wanted.remove(&id) else {
            return;
        };
        let steps = self.steps;
        let mut versions = Vec::new();
        raster.with_rgba(
            |color, alpha| {
                versions = Picture::new(color, alpha, candidate.lossy).store(&candidate, steps);
            },
            None,
        );
        if versions.iter().any(Option::is_some) {
            self.done.insert(id, versions);
        }
    }
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// A decoded picture: 1 (gray) or 3 (RGB) bytes per pixel, and its
/// transparency (`None`: opaque).
struct Picture {
    pixels: Vec<u8>,
    channels: usize,
    width: u32,
    height: u32,
    alpha: Option<LumaData>,
    kind: PictureKind,
}

#[derive(Debug, Clone, PartialEq)]
enum PictureKind {
    Photo,
    /// Few colors: kept pixel for pixel, with a palette (empty for gray).
    Graphic {
        palette: Vec<[u8; 3]>,
    },
}

/// One size of a picture with many colors, stored without loss and with 256
/// colors (computed once for every step that uses this size).
struct Exact {
    pixels: Vec<u8>,
    lossless: Option<Recoded>,
    fewer_colors: Option<Recoded>,
}

impl Picture {
    fn new(color: ImageData, alpha: Option<LumaData>, lossy: bool) -> Self {
        let (mut pixels, mut channels, width, height) = match color {
            ImageData::Rgb(rgb) => (rgb.data, 3, rgb.width, rgb.height),
            ImageData::Luma(luma) => (luma.data, 1, luma.width, luma.height),
        };
        // Color pictures that are gray in fact (scans) lose their color channels.
        let tolerance = if lossy { 6 } else { 0 };
        if channels == 3 && is_gray(&pixels, tolerance) {
            pixels = pixels.as_chunks::<3>().0.iter().map(|p| p[1]).collect();
            channels = 1;
        }
        let alpha = alpha.filter(|a| a.data.iter().any(|v| *v < 255));
        let kind = if lossy {
            PictureKind::Photo
        } else {
            classify(&pixels, channels)
        };
        Self {
            pixels,
            channels,
            width,
            height,
            alpha,
            kind,
        }
    }

    /// The picture stored for each step (`None` where it is not smaller than
    /// the original). Graphics are stored once, pixel for pixel. Other
    /// pictures are scaled to what the step needs on paper, then stored as
    /// JPEG; pictures stored without loss in the file (screenshots, charts)
    /// also try Deflate, exact or with 256 colors, which keeps their edges
    /// sharp: JPEG wins only when it is much smaller.
    fn store(&self, candidate: &Candidate, steps: &[Shrink]) -> Vec<Option<Recoded>> {
        let limit = candidate.stored as f64 * (1.0 - MIN_GAIN);
        let accept = |r: Option<Recoded>| r.filter(|r| (r.len() as f64) < limit);
        if let PictureKind::Graphic { palette } = &self.kind {
            let graphic = accept(self.store_graphic(palette));
            return vec![graphic; steps.len()];
        }
        let mut sizes: HashMap<(u32, u32), Exact> = HashMap::new();
        steps
            .iter()
            .map(|step| {
                let (width, height) = self.size_for(candidate, *step);
                let exact = sizes.entry((width, height)).or_insert_with(|| {
                    let pixels = resample(
                        &self.pixels,
                        self.channels,
                        self.width,
                        self.height,
                        width,
                        height,
                    );
                    let lossless =
                        (!candidate.lossy).then(|| self.store_lossless(&pixels, width, height));
                    let fewer_colors = (!candidate.lossy && self.channels == 3)
                        .then(|| self.store_fewer_colors(&pixels, width, height));
                    Exact {
                        pixels,
                        lossless,
                        fewer_colors,
                    }
                });
                let jpeg = self.store_jpeg(&exact.pixels, width, height, step.quality);
                let mut best = exact.lossless.clone();
                if step.fewer_colors
                    && let Some(fewer) = &exact.fewer_colors
                    && best
                        .as_ref()
                        .is_none_or(|b| (fewer.len() as f64) < b.len() as f64 * 0.9)
                {
                    best = Some(fewer.clone());
                }
                let jpeg_wins = match (&jpeg, &best) {
                    (Some(j), Some(b)) => (j.len() as f64) < b.len() as f64 * 0.75,
                    (Some(_), None) => true,
                    _ => false,
                };
                accept(if jpeg_wins { jpeg } else { best })
            })
            .collect()
    }

    /// Pixels needed on paper at the step's resolution, never more than there are.
    fn size_for(&self, candidate: &Candidate, step: Shrink) -> (u32, u32) {
        let needed = |inches: f32| (inches * step.dpi).ceil().max(1.0) as u32;
        let (want_w, want_h) = (
            needed(candidate.place.width / 72.0),
            needed(candidate.place.height / 72.0),
        );
        let sharper = self.width as f32 > want_w as f32 * RESAMPLE_MARGIN
            || self.height as f32 > want_h as f32 * RESAMPLE_MARGIN;
        if sharper {
            (want_w.min(self.width), want_h.min(self.height))
        } else {
            (self.width, self.height)
        }
    }

    fn device_space(&self) -> &'static str {
        if self.channels == 1 {
            "DeviceGray"
        } else {
            "DeviceRGB"
        }
    }

    fn store_jpeg(&self, pixels: &[u8], width: u32, height: u32, quality: u8) -> Option<Recoded> {
        let jpeg = encode_jpeg(pixels, self.channels, width, height, quality)?;
        let dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width as i64,
            "Height" => height as i64,
            "ColorSpace" => self.device_space(),
            "BitsPerComponent" => 8,
            "Filter" => "DCTDecode",
        };
        Some(Recoded {
            image: Stream::new(dict, jpeg),
            mask: self.store_mask(width, height),
        })
    }

    /// Every pixel kept: Deflate after the PNG filter that suits each row.
    fn store_lossless(&self, pixels: &[u8], width: u32, height: u32) -> Recoded {
        let filtered = png_filter(pixels, self.channels, width as usize);
        let dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width as i64,
            "Height" => height as i64,
            "ColorSpace" => self.device_space(),
            "BitsPerComponent" => 8,
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! {
                "Predictor" => 15,
                "Colors" => self.channels as i64,
                "BitsPerComponent" => 8,
                "Columns" => width as i64,
            },
        };
        Recoded {
            image: Stream::new(dict, deflate(&filtered)),
            mask: self.store_mask(width, height),
        }
    }

    /// The 256 colors that represent the picture best (no dithering: flat
    /// areas stay flat, so they compress well).
    fn store_fewer_colors(&self, pixels: &[u8], width: u32, height: u32) -> Recoded {
        let (palette, indices) = quantize(pixels, 256);
        Recoded {
            image: indexed(&indices, width, height, &palette, false),
            mask: self.store_mask(width, height),
        }
    }

    /// Few colors: one byte or less per pixel through a palette, or gray levels.
    fn store_graphic(&self, palette: &[[u8; 3]]) -> Option<Recoded> {
        let (indices, colors) = if self.channels == 1 {
            let mut levels: Vec<u8> = self.pixels.clone();
            levels.sort_unstable();
            levels.dedup();
            let mut index = [0u8; 256];
            for (i, v) in levels.iter().enumerate() {
                index[*v as usize] = i as u8;
            }
            let indices: Vec<u8> = self.pixels.iter().map(|v| index[*v as usize]).collect();
            (
                indices,
                levels.iter().map(|v| [*v, *v, *v]).collect::<Vec<_>>(),
            )
        } else {
            let index: HashMap<[u8; 3], u8> = palette
                .iter()
                .enumerate()
                .map(|(i, c)| (*c, i as u8))
                .collect();
            let indices = self
                .pixels
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| index.get(&[p[0], p[1], p[2]]).copied())
                .collect::<Option<Vec<u8>>>()?;
            (indices, palette.to_vec())
        };
        Some(Recoded {
            image: indexed(
                &indices,
                self.width,
                self.height,
                &colors,
                self.channels == 1,
            ),
            mask: self.store_mask(self.width, self.height),
        })
    }

    /// Transparency as an 8-bit gray soft mask, no sharper than the picture.
    fn store_mask(&self, width: u32, height: u32) -> Option<Stream> {
        let alpha = self.alpha.as_ref()?;
        let (w, h) = (width.min(alpha.width), height.min(alpha.height));
        let values = resample(&alpha.data, 1, alpha.width, alpha.height, w, h);
        let filtered = png_filter(&values, 1, w as usize);
        let dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => w as i64,
            "Height" => h as i64,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! {
                "Predictor" => 15,
                "Colors" => 1,
                "BitsPerComponent" => 8,
                "Columns" => w as i64,
            },
        };
        Some(Stream::new(dict, deflate(&filtered)))
    }
}

/// A picture through a palette of `colors` (gray levels when `gray`), with
/// as few bits per pixel as the palette allows.
fn indexed(indices: &[u8], width: u32, height: u32, colors: &[[u8; 3]], gray: bool) -> Stream {
    let bits = match colors.len() {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        _ => 8,
    };
    let packed = pack(indices, width as usize, bits);
    let color_space = if gray && is_identity_gray(colors, bits) {
        Object::Name(b"DeviceGray".to_vec())
    } else {
        let base = if gray { "DeviceGray" } else { "DeviceRGB" };
        let table: Vec<u8> = if gray {
            colors.iter().map(|c| c[0]).collect()
        } else {
            colors.iter().flatten().copied().collect()
        };
        Object::Array(vec![
            Object::Name(b"Indexed".to_vec()),
            Object::Name(base.as_bytes().to_vec()),
            Object::Integer(colors.len() as i64 - 1),
            Object::String(table, lopdf::StringFormat::Hexadecimal),
        ])
    };
    let dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Image",
        "Width" => width as i64,
        "Height" => height as i64,
        "ColorSpace" => color_space,
        "BitsPerComponent" => bits as i64,
        "Filter" => "FlateDecode",
    };
    Stream::new(dict, deflate(&packed))
}

/// PNG filtering (PDF predictor 15): each row gets the filter whose output
/// is smallest in absolute value, a good guess of what Deflate packs best.
pub(crate) fn png_filter(pixels: &[u8], bpp: usize, width: usize) -> Vec<u8> {
    let stride = width * bpp;
    if stride == 0 {
        return Vec::new();
    }
    let rows = pixels.len() / stride;
    let zero = vec![0u8; stride];
    let mut out = Vec::with_capacity((stride + 1) * rows);
    let mut trial = vec![0u8; stride];
    let mut best = vec![0u8; stride];
    for y in 0..rows {
        let row = &pixels[y * stride..(y + 1) * stride];
        let up = if y == 0 {
            &zero[..]
        } else {
            &pixels[(y - 1) * stride..y * stride]
        };
        let mut best_filter = 0u8;
        let mut best_score = u64::MAX;
        for filter in 0..5u8 {
            for i in 0..stride {
                let left = if i >= bpp { row[i - bpp] } else { 0 };
                let above = up[i];
                let corner = if i >= bpp { up[i - bpp] } else { 0 };
                let predicted = match filter {
                    0 => 0,
                    1 => left,
                    2 => above,
                    3 => ((left as u16 + above as u16) / 2) as u8,
                    _ => paeth(left, above, corner),
                };
                trial[i] = row[i].wrapping_sub(predicted);
            }
            let score: u64 = trial.iter().map(|v| (*v as i8).unsigned_abs() as u64).sum();
            if score < best_score {
                best_score = score;
                best_filter = filter;
                best.copy_from_slice(&trial);
            }
        }
        out.push(best_filter);
        out.extend_from_slice(&best);
    }
    out
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = (
        (p - a as i16).abs(),
        (p - b as i16).abs(),
        (p - c as i16).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Median cut: at most `max` colors standing for an RGB picture, and each
/// pixel's color among them. Colors are grouped 6 bits per channel.
pub(crate) fn quantize(rgb: &[u8], max: usize) -> (Vec<[u8; 3]>, Vec<u8>) {
    const BINS: usize = 1 << 18;
    let bin =
        |p: &[u8]| (p[0] as usize >> 2) << 12 | (p[1] as usize >> 2) << 6 | p[2] as usize >> 2;
    let mut count = vec![0u32; BINS];
    let mut sum = vec![[0u64; 3]; BINS];
    for p in rgb.as_chunks::<3>().0.iter() {
        let b = bin(p);
        count[b] += 1;
        for c in 0..3 {
            sum[b][c] += p[c] as u64;
        }
    }
    let mut bins: Vec<usize> = (0..BINS).filter(|b| count[*b] > 0).collect();
    let axis = |b: usize, a: usize| (b >> (12 - 6 * a)) & 63;
    let mut boxes: Vec<(usize, usize)> = vec![(0, bins.len())];
    while boxes.len() < max {
        // The box spread the most, weighted by its pixels, is split in two.
        let mut chosen: Option<(usize, usize, u64)> = None;
        for (i, &(start, end)) in boxes.iter().enumerate() {
            if end - start < 2 {
                continue;
            }
            let pixels: u64 = bins[start..end].iter().map(|b| count[*b] as u64).sum();
            for a in 0..3 {
                let (lo, hi) = bins[start..end].iter().fold((63, 0), |(lo, hi), b| {
                    (lo.min(axis(*b, a)), hi.max(axis(*b, a)))
                });
                let score = pixels * (hi - lo) as u64;
                if score > 0 && chosen.is_none_or(|(_, _, s)| score > s) {
                    chosen = Some((i, a, score));
                }
            }
        }
        let Some((i, a, _)) = chosen else { break };
        let (start, end) = boxes[i];
        bins[start..end].sort_unstable_by_key(|b| axis(*b, a));
        let half: u64 = bins[start..end]
            .iter()
            .map(|b| count[*b] as u64)
            .sum::<u64>()
            / 2;
        let mut seen = 0u64;
        let mut cut = start + 1;
        for (k, b) in bins[start..end].iter().enumerate() {
            seen += count[*b] as u64;
            if seen >= half {
                cut = (start + k + 1).clamp(start + 1, end - 1);
                break;
            }
        }
        boxes[i] = (start, cut);
        boxes.push((cut, end));
    }
    let palette: Vec<[u8; 3]> = boxes
        .iter()
        .map(|&(start, end)| {
            let n: u64 = bins[start..end]
                .iter()
                .map(|b| count[*b] as u64)
                .sum::<u64>()
                .max(1);
            let channel =
                |c: usize| (bins[start..end].iter().map(|b| sum[*b][c]).sum::<u64>() / n) as u8;
            [channel(0), channel(1), channel(2)]
        })
        .collect();
    // Each group of colors goes to the nearest color of the palette.
    let mut index = vec![0u8; BINS];
    for &b in &bins {
        let n = count[b] as u64;
        let mean = [0, 1, 2].map(|c| (sum[b][c] / n) as i32);
        let nearest = palette
            .iter()
            .enumerate()
            .min_by_key(|(_, p)| (0..3).map(|c| (p[c] as i32 - mean[c]).pow(2)).sum::<i32>())
            .map_or(0, |(i, _)| i);
        index[b] = nearest as u8;
    }
    let indices = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| index[bin(p)])
        .collect();
    (palette, indices)
}

/// The palette is plain gray levels 0…max: no palette needed.
fn is_identity_gray(levels: &[[u8; 3]], bits: u32) -> bool {
    let max = (1u32 << bits) - 1;
    levels.len() as u32 == max + 1
        && levels
            .iter()
            .enumerate()
            .all(|(i, c)| u32::from(c[0]) == i as u32 * 255 / max)
}

fn is_gray(rgb: &[u8], tolerance: u8) -> bool {
    rgb.as_chunks::<3>().0.iter().all(|p| {
        let (lo, hi) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
        hi - lo <= tolerance
    })
}

/// Photo or graphic, by the number of distinct colors.
fn classify(pixels: &[u8], channels: usize) -> PictureKind {
    if channels == 1 {
        let mut seen = [false; 256];
        for v in pixels {
            seen[*v as usize] = true;
        }
        let levels = seen.iter().filter(|s| **s).count();
        return if levels <= GRAPHIC_GRAYS {
            PictureKind::Graphic {
                palette: Vec::new(),
            }
        } else {
            PictureKind::Photo
        };
    }
    let mut colors: HashSet<[u8; 3]> = HashSet::new();
    for p in pixels.as_chunks::<3>().0.iter() {
        colors.insert([p[0], p[1], p[2]]);
        if colors.len() > GRAPHIC_COLORS {
            return PictureKind::Photo;
        }
    }
    let mut palette: Vec<[u8; 3]> = colors.into_iter().collect();
    palette.sort_unstable();
    PictureKind::Graphic { palette }
}

/// Indices packed `bits` per pixel, each row starting on a new byte.
fn pack(indices: &[u8], width: usize, bits: u32) -> Vec<u8> {
    if bits == 8 {
        return indices.to_vec();
    }
    let per_byte = 8 / bits as usize;
    let row_bytes = width.div_ceil(per_byte);
    let mut out = Vec::with_capacity(row_bytes * indices.len() / width.max(1));
    for row in indices.chunks(width.max(1)) {
        for group in row.chunks(per_byte) {
            let mut byte = 0u8;
            for (i, v) in group.iter().enumerate() {
                byte |= v << (8 - bits as usize * (i + 1));
            }
            out.push(byte);
        }
    }
    out
}

/// Smaller picture by averaging the pixels each new pixel covers (box
/// filter): sharp and without ringing when scaling down. Never scales up.
pub(crate) fn resample(
    pixels: &[u8],
    channels: usize,
    width: u32,
    height: u32,
    new_width: u32,
    new_height: u32,
) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let (nw, nh) = (
        new_width.clamp(1, width) as usize,
        new_height.clamp(1, height) as usize,
    );
    if (nw, nh) == (w, h) {
        return pixels.to_vec();
    }
    // Weights of the source columns (or rows) covered by each new one.
    let spans = |from: usize, to: usize| -> Vec<Vec<(usize, f32)>> {
        let scale = from as f32 / to as f32;
        (0..to)
            .map(|i| {
                let (start, end) = (i as f32 * scale, (i + 1) as f32 * scale);
                let mut weights = Vec::new();
                let mut x = start.floor() as usize;
                while (x as f32) < end && x < from {
                    let covered = (end.min(x as f32 + 1.0) - start.max(x as f32)).max(0.0);
                    if covered > 0.0 {
                        weights.push((x, covered / scale));
                    }
                    x += 1;
                }
                weights
            })
            .collect()
    };
    let columns = spans(w, nw);
    let rows = spans(h, nh);
    // Horizontal pass into floats, then vertical pass.
    let mut horizontal = vec![0f32; nw * h * channels];
    for y in 0..h {
        let src = &pixels[y * w * channels..(y + 1) * w * channels];
        let dst = &mut horizontal[y * nw * channels..(y + 1) * nw * channels];
        for (nx, weights) in columns.iter().enumerate() {
            for c in 0..channels {
                dst[nx * channels + c] = weights
                    .iter()
                    .map(|(x, wgt)| src[x * channels + c] as f32 * wgt)
                    .sum();
            }
        }
    }
    let mut out = vec![0u8; nw * nh * channels];
    let stride = nw * channels;
    for (ny, weights) in rows.iter().enumerate() {
        let dst = &mut out[ny * stride..(ny + 1) * stride];
        for (i, value) in dst.iter_mut().enumerate() {
            let sum: f32 = weights
                .iter()
                .map(|(y, wgt)| horizontal[y * stride + i] * wgt)
                .sum();
            *value = sum.round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// Progressive JPEG with optimized tables (gray or RGB, 4:2:0 color).
pub(crate) fn encode_jpeg(
    pixels: &[u8],
    channels: usize,
    width: u32,
    height: u32,
    quality: u8,
) -> Option<Vec<u8>> {
    let (w, h) = (u16::try_from(width).ok()?, u16::try_from(height).ok()?);
    let mut out = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut out, quality.clamp(1, 100));
    encoder.set_progressive(true);
    encoder.set_optimized_huffman_tables(true);
    encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_2_0);
    let color = if channels == 1 {
        jpeg_encoder::ColorType::Luma
    } else {
        jpeg_encoder::ColorType::Rgb
    };
    encoder.encode(pixels, w, h, color).ok()?;
    Some(out)
}

pub(crate) fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::with_capacity(data.len() / 2), Compression::best());
    encoder.write_all(data).expect("writing to memory");
    encoder.finish().expect("writing to memory")
}

fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len() * 3);
    ZlibDecoder::new(data).read_to_end(&mut out).ok()?;
    Some(out)
}

// --- The document ----------------------------------------------------------------------

/// The source opened with lopdf, cleaned once (metadata, unused objects,
/// duplicates, recompressed streams) and copied for each attempt.
struct Base {
    doc: Document,
}

impl Base {
    fn open(data: &[u8]) -> Result<Self, CoreError> {
        // hayro checks the file first: it tells a password-protected PDF apart.
        pdf::open(data.to_vec())?;
        let mut doc = Document::load_mem(data).map_err(|e| match e {
            lopdf::Error::Decryption(_) => CoreError::PdfEncrypted,
            other => CoreError::CorruptInput {
                details: format!("PDF: {other}"),
            },
        })?;
        if doc.is_encrypted() {
            doc.decrypt("").map_err(|_| CoreError::PdfEncrypted)?;
        }
        doc.trailer.remove(b"Encrypt");
        doc.trailer.remove(b"ID");
        let info = doc.add_object(dictionary! { "Producer" => Object::string_literal("Konvert") });
        doc.trailer.set("Info", info);
        drop_metadata(&mut doc);
        keep_reachable(&mut doc);
        merge_duplicates(&mut doc);
        // Lossless: every level keeps only the glyphs the document shows.
        crate::pdf_fonts::subset_fonts(&mut doc);
        recompress_streams(&mut doc);
        Ok(Self { doc })
    }

    /// A picture worth decoding: a plain image (not a mask, nor a soft mask
    /// of another one), drawn somewhere.
    fn candidate(&self, id: ObjectId, place: Placed) -> Option<Candidate> {
        let Ok(Object::Stream(stream)) = self.doc.get_object(id) else {
            return None;
        };
        let dict = &stream.dict;
        let is = |key: &[u8], value: &[u8]| {
            dict.get(key)
                .and_then(Object::as_name)
                .is_ok_and(|v| v == value)
        };
        if !is(b"Subtype", b"Image")
            || dict
                .get(b"ImageMask")
                .and_then(Object::as_bool)
                .unwrap_or(false)
            || self.soft_masks().contains(&id)
        {
            return None;
        }
        // Soft masks that carry a matte color would need it removed from the
        // picture first: such pictures stay as they are.
        let mask = match dict.get(b"SMask") {
            Ok(Object::Reference(mask_id)) => match self.doc.get_object(*mask_id) {
                Ok(Object::Stream(mask)) if mask.dict.has(b"Matte") => return None,
                Ok(Object::Stream(mask)) => mask.content.len(),
                _ => 0,
            },
            _ => 0,
        };
        let filters = filters(dict);
        if filters
            .iter()
            .any(|f| matches!(f.as_slice(), b"JBIG2Decode" | b"CCITTFaxDecode"))
        {
            // Black-and-white scans are already stored in the best way.
            return None;
        }
        let lossy = filters
            .iter()
            .any(|f| matches!(f.as_slice(), b"DCTDecode" | b"JPXDecode"));
        Some(Candidate {
            place,
            stored: stream.content.len() + mask,
            lossy,
        })
    }

    fn soft_masks(&self) -> HashSet<ObjectId> {
        self.doc
            .objects
            .values()
            .filter_map(|o| match o {
                Object::Stream(s) => s.dict.get(b"SMask").and_then(Object::as_reference).ok(),
                _ => None,
            })
            .collect()
    }

    /// The document with the pictures of `step`, as file bytes.
    fn write(&self, versions: &Versions, step: usize) -> Result<Vec<u8>, CoreError> {
        let mut doc = self.doc.clone();
        for (id, steps) in versions {
            let Some(Some(recoded)) = steps.get(step) else {
                continue;
            };
            let Ok(Object::Stream(old)) = doc.get_object(*id) else {
                continue;
            };
            // Keep what the picture's entries say beyond its pixels.
            let mut dict = old.dict.clone();
            for key in [
                &b"Filter"[..],
                b"DecodeParms",
                b"Decode",
                b"ColorSpace",
                b"BitsPerComponent",
                b"Width",
                b"Height",
                b"Mask",
                b"SMask",
                b"SMaskInData",
                b"Alternates",
                b"Length",
                b"Metadata",
            ] {
                dict.remove(key);
            }
            for (key, value) in recoded.image.dict.iter() {
                dict.set(key.clone(), value.clone());
            }
            if let Some(mask) = &recoded.mask {
                let mask_id = doc.add_object(mask.clone());
                dict.set("SMask", mask_id);
            }
            doc.objects.insert(
                *id,
                Object::Stream(
                    Stream::new(dict, recoded.image.content.clone()).with_compression(false),
                ),
            );
        }
        keep_reachable(&mut doc);
        save(&mut doc)
    }
}

/// Saves a document with its small objects packed into object streams and a
/// compressed cross-reference stream.
pub(crate) fn save(doc: &mut Document) -> Result<Vec<u8>, CoreError> {
    let mut bytes = Vec::new();
    doc.save_with_options(
        &mut bytes,
        SaveOptions::builder()
            .use_object_streams(true)
            .use_xref_streams(true)
            .max_objects_per_stream(500)
            .compression_level(9)
            .build(),
    )
    .map_err(|e| CoreError::Io(std::io::Error::other(e.to_string())))?;
    Ok(pack_xref_stream(&bytes).unwrap_or(bytes))
}

/// lopdf writes the cross-reference stream (the last object of the file)
/// uncompressed: it is written again in place with Deflate and the PNG "up"
/// predictor, as Acrobat does. Its position does not change, so neither does
/// any offset. `None` if the file does not end the way lopdf writes it.
fn pack_xref_stream(bytes: &[u8]) -> Option<Vec<u8>> {
    let find =
        |haystack: &[u8], needle: &[u8]| haystack.windows(needle.len()).position(|w| w == needle);
    let tail_from = bytes.len().saturating_sub(48);
    let at = tail_from + find(&bytes[tail_from..], b"startxref")?;
    let start: usize = std::str::from_utf8(&bytes[at + 9..])
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let object = bytes.get(start..at)?;
    let dict_start = find(object, b"<<")?;
    let dict_end = find(object, b">>stream\n")?;
    let dict = std::str::from_utf8(&object[dict_start..dict_end]).ok()?;
    if dict.contains("/Filter") || !dict.contains("/Type/XRef") {
        return None;
    }
    let number_after = |key: &str| -> Option<usize> {
        let rest = &dict[dict.find(key)? + key.len()..];
        let digits: String = rest
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    };
    let length = number_after("/Length")?;
    let widths = &dict[dict.find("/W[")? + 3..];
    let columns: usize = widths[..widths.find(']')?]
        .split_whitespace()
        .map(|w| w.parse::<usize>().ok())
        .sum::<Option<usize>>()?;
    let content = object.get(dict_end + 9..dict_end + 9 + length)?;
    if columns == 0 || content.len() % columns != 0 {
        return None;
    }
    let mut filtered = Vec::with_capacity(content.len() + content.len() / columns);
    let mut above = vec![0u8; columns];
    for row in content.chunks_exact(columns) {
        filtered.push(2);
        filtered.extend(row.iter().zip(&above).map(|(v, u)| v.wrapping_sub(*u)));
        above.copy_from_slice(row);
    }
    let packed = deflate(&filtered);
    let length_text = format!("/Length {length}");
    if !dict.contains(&length_text) {
        return None;
    }
    let new_dict = dict.replace(
        &length_text,
        &format!(
            "/Length {}/Filter/FlateDecode/DecodeParms<</Predictor 12/Columns {columns}>>",
            packed.len()
        ),
    );
    let mut out = Vec::with_capacity(start + packed.len() + 256);
    out.extend_from_slice(&bytes[..start + dict_start]);
    out.extend_from_slice(new_dict.as_bytes());
    out.extend_from_slice(b">>stream\n");
    out.extend_from_slice(&packed);
    out.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{start}\n%%EOF\n").as_bytes());
    Some(out)
}

/// Filter names of a stream, in order.
fn filters(dict: &Dictionary) -> Vec<Vec<u8>> {
    match dict.get(b"Filter") {
        Ok(Object::Name(name)) => vec![name.clone()],
        Ok(Object::Array(list)) => list
            .iter()
            .filter_map(|f| f.as_name().ok().map(<[u8]>::to_vec))
            .collect(),
        _ => Vec::new(),
    }
}

/// Removes XMP metadata, page thumbnails and private data of the programs
/// that made the file.
fn drop_metadata(doc: &mut Document) {
    for object in doc.objects.values_mut() {
        let dict = match object {
            Object::Dictionary(dict) => dict,
            Object::Stream(stream) => &mut stream.dict,
            _ => continue,
        };
        for key in [&b"Metadata"[..], b"PieceInfo", b"Thumb"] {
            dict.remove(key);
        }
    }
}

/// Keeps only the objects the document reaches from its trailer.
pub(crate) fn keep_reachable(doc: &mut Document) {
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut stack: Vec<ObjectId> = Vec::new();
    collect_references(&Object::Dictionary(doc.trailer.clone()), &mut stack);
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(object) = doc.objects.get(&id) {
            collect_references(object, &mut stack);
        }
    }
    doc.objects.retain(|id, _| seen.contains(id));
}

fn collect_references(object: &Object, found: &mut Vec<ObjectId>) {
    match object {
        Object::Reference(id) => found.push(*id),
        Object::Array(items) => items.iter().for_each(|o| collect_references(o, found)),
        Object::Dictionary(dict) => dict.iter().for_each(|(_, o)| collect_references(o, found)),
        Object::Stream(stream) => stream
            .dict
            .iter()
            .for_each(|(_, o)| collect_references(o, found)),
        _ => {}
    }
}

/// Identical streams (the same picture, font or color profile stored twice)
/// become one.
fn merge_duplicates(doc: &mut Document) {
    let mut by_hash: HashMap<u64, Vec<ObjectId>> = HashMap::new();
    for (id, object) in &doc.objects {
        if let Object::Stream(stream) = object {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            hash_dict(&stream.dict, &mut hasher);
            stream.content.hash(&mut hasher);
            by_hash.entry(hasher.finish()).or_default().push(*id);
        }
    }
    let mut replace: HashMap<ObjectId, ObjectId> = HashMap::new();
    for mut ids in by_hash.into_values().filter(|ids| ids.len() > 1) {
        ids.sort_unstable();
        let mut kept: Vec<ObjectId> = Vec::new();
        for id in ids {
            let same = kept.iter().copied().find(|k| same_stream(doc, *k, id));
            match same {
                Some(k) => {
                    replace.insert(id, k);
                }
                None => kept.push(id),
            }
        }
    }
    if replace.is_empty() {
        return;
    }
    for object in doc.objects.values_mut() {
        redirect(object, &replace);
    }
    for id in replace.keys() {
        doc.objects.remove(id);
    }
}

fn same_stream(doc: &Document, a: ObjectId, b: ObjectId) -> bool {
    match (doc.objects.get(&a), doc.objects.get(&b)) {
        (Some(Object::Stream(x)), Some(Object::Stream(y))) => {
            x.content == y.content && without_length(&x.dict) == without_length(&y.dict)
        }
        _ => false,
    }
}

fn without_length(dict: &Dictionary) -> Dictionary {
    let mut dict = dict.clone();
    dict.remove(b"Length");
    dict
}

fn redirect(object: &mut Object, replace: &HashMap<ObjectId, ObjectId>) {
    match object {
        Object::Reference(id) => {
            if let Some(new) = replace.get(id) {
                *id = *new;
            }
        }
        Object::Array(items) => items.iter_mut().for_each(|o| redirect(o, replace)),
        Object::Dictionary(dict) => dict.iter_mut().for_each(|(_, o)| redirect(o, replace)),
        Object::Stream(stream) => stream
            .dict
            .iter_mut()
            .for_each(|(_, o)| redirect(o, replace)),
        _ => {}
    }
}

fn hash_dict(dict: &Dictionary, hasher: &mut impl Hasher) {
    for (key, value) in dict.iter() {
        if key.as_slice() == b"Length" {
            continue;
        }
        key.hash(hasher);
        hash_object(value, hasher);
    }
}

fn hash_object(object: &Object, hasher: &mut impl Hasher) {
    std::mem::discriminant(object).hash(hasher);
    match object {
        Object::Boolean(b) => b.hash(hasher),
        Object::Integer(i) => i.hash(hasher),
        Object::Real(r) => r.to_bits().hash(hasher),
        Object::Name(n) => n.hash(hasher),
        Object::String(s, _) => s.hash(hasher),
        Object::Array(items) => items.iter().for_each(|o| hash_object(o, hasher)),
        Object::Dictionary(dict) => hash_dict(dict, hasher),
        Object::Stream(stream) => {
            hash_dict(&stream.dict, hasher);
            stream.content.hash(hasher);
        }
        Object::Reference(id) => id.hash(hasher),
        Object::Null => {}
    }
}

/// Every uncompressed or Deflate-compressed stream is compressed again with
/// the strongest setting, when that is smaller.
fn recompress_streams(doc: &mut Document) {
    for object in doc.objects.values_mut() {
        let Object::Stream(stream) = object else {
            continue;
        };
        let filters = filters(&stream.dict);
        let raw = match filters.as_slice() {
            [] => Some(stream.content.clone()),
            [only] if only == b"FlateDecode" => inflate(&stream.content),
            _ => None,
        };
        let Some(raw) = raw else { continue };
        let packed = deflate(&raw);
        if packed.len() < stream.content.len() {
            if filters.is_empty() {
                stream.dict.set("Filter", "FlateDecode");
                stream.dict.remove(b"DecodeParms");
            }
            stream.set_content(packed);
            stream.allows_compression = false;
        }
    }
}

// --- Pages as pictures -----------------------------------------------------------------

/// Every page drawn on white and stored as a JPEG picture of the same size.
fn pages_as_pictures(data: &[u8], step: Shrink, cancel: &AtomicBool) -> Result<Vec<u8>, CoreError> {
    let pdf = pdf::open(data.to_vec())?;
    let cache = RenderCache::new();
    let settings = InterpreterSettings::default();
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();
    for page in pdf.pages().iter() {
        if cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        let (width_pt, height_pt) = page.render_dimensions();
        let scale = pdf::render_scale(width_pt, height_pt, step.dpi as u32);
        let pixmap =
            pdf::draw(page, &cache, &settings, scale).ok_or_else(|| CoreError::CorruptInput {
                details: "PDF: a page could not be drawn".into(),
            })?;
        let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
        let rgb: Vec<u8> = pixmap
            .data_as_u8_slice()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        let (pixels, channels) = if is_gray(&rgb, 2) {
            (rgb.as_chunks::<3>().0.iter().map(|p| p[1]).collect(), 1)
        } else {
            (rgb, 3)
        };
        let jpeg =
            encode_jpeg(&pixels, channels, width, height, step.quality).ok_or_else(|| {
                CoreError::Io(std::io::Error::other(
                    "the page picture could not be encoded",
                ))
            })?;
        let image = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => width as i64,
                "Height" => height as i64,
                "ColorSpace" => if channels == 1 { "DeviceGray" } else { "DeviceRGB" },
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            jpeg,
        ));
        let content = format!("q {width_pt:.3} 0 0 {height_pt:.3} 0 0 cm /Im0 Do Q");
        let content = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
        let page = dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), width_pt.into(), height_pt.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } },
            "Contents" => content,
        };
        kids.push(Object::Reference(doc.add_object(page)));
    }
    let count = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    let info = doc.add_object(dictionary! { "Producer" => Object::string_literal("Konvert") });
    doc.trailer.set("Root", catalog);
    doc.trailer.set("Info", info);
    save(&mut doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_are_averaged_down() {
        // 4×2 gray → 2×1: each new pixel averages a 2×2 block.
        let pixels = [0, 100, 200, 255, 0, 100, 200, 255];
        assert_eq!(resample(&pixels, 1, 4, 2, 2, 1), [50, 228]);
        // Never larger.
        assert_eq!(resample(&pixels, 1, 4, 2, 8, 4), pixels);
        // 3 → 2 columns: fractional coverage.
        let row = [0, 90, 180];
        assert_eq!(resample(&row, 1, 3, 1, 2, 1), [30, 150]);
        let rgb = [10, 20, 30, 30, 40, 50];
        assert_eq!(resample(&rgb, 3, 2, 1, 1, 1), [20, 30, 40]);
    }

    #[test]
    fn graphics_and_photos_are_told_apart() {
        let two_colors: Vec<u8> = (0..300)
            .flat_map(|i| if i % 2 == 0 { [255, 0, 0] } else { [0, 0, 255] })
            .collect();
        match classify(&two_colors, 3) {
            PictureKind::Graphic { palette } => assert_eq!(palette, [[0, 0, 255], [255, 0, 0]]),
            other => panic!("{other:?}"),
        }
        let photo: Vec<u8> = (0..1000u32)
            .flat_map(|i| [(i % 256) as u8, (i / 4 % 256) as u8, 7])
            .collect();
        assert_eq!(classify(&photo, 3), PictureKind::Photo);
        let gray_ramp: Vec<u8> = (0..=255).collect();
        assert_eq!(classify(&gray_ramp, 1), PictureKind::Photo);
        assert!(matches!(
            classify(&[0, 255, 0, 255], 1),
            PictureKind::Graphic { .. }
        ));
        assert!(is_gray(&[10, 12, 9, 200, 200, 201], 3));
        assert!(!is_gray(&[10, 12, 9, 200, 200, 201], 0));
    }

    #[test]
    fn indices_are_packed_per_row() {
        // Two rows of three 1-bit pixels: each row on its own byte.
        assert_eq!(pack(&[1, 0, 1, 0, 1, 1], 3, 1), [0b1010_0000, 0b0110_0000]);
        assert_eq!(pack(&[3, 1, 2], 3, 2), [0b1101_1000]);
        assert_eq!(pack(&[15, 1], 2, 4), [0xF1]);
        assert!(is_identity_gray(&[[0; 3], [255; 3]], 1));
        assert!(!is_identity_gray(&[[0; 3], [128; 3]], 1));
    }

    #[test]
    fn placed_size_follows_the_transform() {
        // A 300×150 picture drawn 3 inches wide (216 pt), turned a quarter.
        let t = kurbo::Affine::new([0.0, -0.72, 0.72, 0.0, 0.0, 0.0]);
        let placed = placed_size(t, 300, 150);
        assert!((placed.width - 216.0).abs() < 0.01 && (placed.height - 108.0).abs() < 0.01);
    }

    #[test]
    fn photos_become_small_jpegs() {
        let (w, h) = (64u32, 48u32);
        let pixels: Vec<u8> = (0..w * h)
            .flat_map(|i| [(i % 64 * 4) as u8, (i / 64 * 5) as u8, 90])
            .collect();
        let jpeg = encode_jpeg(&pixels, 3, w, h, 50).unwrap();
        assert!(jpeg.starts_with(&[0xFF, 0xD8]) && jpeg.len() < pixels.len() / 4);
        assert_eq!(
            inflate(&deflate(b"hello hello hello")).unwrap(),
            b"hello hello hello"
        );
    }
}
