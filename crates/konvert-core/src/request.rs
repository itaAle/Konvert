//! What the user asks for: target format, quality preset and advanced options.
//!
//! These types are the whole contract between the UI and the core: the UI never
//! sends FFmpeg arguments, only a [`ConversionRequest`].

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::formats::{AudioCodec, TargetFormat, VideoCodec};
use crate::secret::Secret;

/// Plain-language quality choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum QualityPreset {
    /// "Massima qualità"
    Max,
    /// "Bilanciata"
    #[default]
    Balanced,
    /// "File piccolo"
    Small,
}

/// Channel layout choice in the advanced options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ChannelLayout {
    Mono,
    Stereo,
}

impl ChannelLayout {
    pub fn count(self) -> u32 {
        match self {
            ChannelLayout::Mono => 1,
            ChannelLayout::Stereo => 2,
        }
    }
}

/// Optional overrides of the preset ("Avanzate" section). `None` = automatic.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, optional_fields = nullable)]
pub struct AdvancedOptions {
    /// Audio bit rate in kbit/s (lossy codecs only).
    pub audio_bitrate_kbps: Option<u32>,
    /// Output sample rate in Hz.
    pub sample_rate: Option<u32>,
    pub channels: Option<ChannelLayout>,
    /// Audio encoder, among those allowed by the target format.
    pub audio_codec: Option<AudioCodec>,
    /// Maximum output height in pixels (never upscales; keeps the aspect ratio).
    pub max_height: Option<u32>,
    /// Output frame rate.
    pub fps: Option<f64>,
    /// Video encoder, among those allowed by the target format.
    pub video_codec: Option<VideoCodec>,
    /// Video bit rate in kbit/s (replaces the preset's quality setting).
    pub video_bitrate_kbps: Option<u32>,
    /// Picture quality from 1 (smallest file) to 100 (best), lossy picture
    /// formats only.
    pub image_quality: Option<u8>,
    /// Longest side of a picture in pixels (never upscales; keeps the aspect ratio).
    pub max_dimension: Option<u32>,
}

impl AdvancedOptions {
    /// Any option that requires re-encoding the audio stream.
    pub fn touches_audio(&self) -> bool {
        self.audio_bitrate_kbps.is_some()
            || self.sample_rate.is_some()
            || self.channels.is_some()
            || self.audio_codec.is_some()
    }

    /// Any option that requires re-encoding the video stream.
    pub fn touches_video(&self) -> bool {
        self.max_height.is_some()
            || self.fps.is_some()
            || self.video_codec.is_some()
            || self.video_bitrate_kbps.is_some()
    }
}

/// Pictures taken from a video: one frame, or a series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FrameMode {
    /// One picture at a chosen moment.
    #[default]
    Single,
    /// A picture at regular intervals, saved in a folder.
    Sequence,
}

/// How pictures are taken from a video (picture targets of a video input).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, optional_fields = nullable)]
pub struct FrameOptions {
    pub mode: FrameMode,
    /// One frame: position in seconds (`None`: the middle of the video).
    pub at_secs: Option<f64>,
    /// Series: one picture every `every_secs` seconds (`None`: every frame).
    pub every_secs: Option<f64>,
}

/// Paper size of the pages made from pictures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PageSize {
    #[default]
    A4,
    Letter,
    /// Shaped like the picture.
    Fit,
}

/// Orientation of the pages made from pictures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PageOrientation {
    /// Turned to match each picture.
    #[default]
    Auto,
    Portrait,
    Landscape,
}

/// White border around the pictures on their pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PageMargin {
    None,
    #[default]
    Small,
    Normal,
}

/// A page of a PDF made from other files: which page, and how it is turned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PdfPage {
    /// Index of the source file among the files combined (0 for a single PDF).
    #[serde(default)]
    pub file: u32,
    /// 1-based page number in the source file (a picture has one page).
    pub number: u32,
    /// Clockwise turn added to the page's own: 0, 90, 180 or 270 degrees.
    #[serde(default)]
    pub rotation: u16,
}

/// How an edited PDF is saved: one document, or several.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PdfSplit {
    /// One document.
    #[default]
    None,
    /// A new document every `split_every` pages.
    Every,
    /// A new document after each position listed in `split_after`.
    After,
}

/// A password that protects a PDF made by Konvert (AES-256), and what readers
/// may do with it once opened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PdfProtection {
    /// Opens the document. Never printed or written out (see [`Secret`]).
    #[ts(type = "string")]
    pub password: Secret,
    #[serde(default = "yes")]
    pub allow_print: bool,
    /// Copying its text (always allowed to accessibility tools).
    #[serde(default = "yes")]
    pub allow_copy: bool,
}

fn yes() -> bool {
    true
}

/// Where something is drawn on a page, as the page is shown (after its turn).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// Text or a picture drawn over the pages of a new PDF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, optional_fields = nullable)]
pub struct Watermark {
    /// The text (any language), unless a picture is given.
    pub text: String,
    /// A PNG or JPEG picture drawn instead of the text.
    #[ts(type = "string | null")]
    pub picture: Option<std::path::PathBuf>,
    /// Size of the text in points; width of a picture in percent of the page.
    pub size: f32,
    /// Color of the text, `#rrggbb`.
    pub color: String,
    /// From 0.05 (barely visible) to 1 (opaque).
    pub opacity: f32,
    /// Counterclockwise turn in degrees, from -90 to 90.
    pub angle: i32,
    pub position: Anchor,
    /// Pages of each new document that show it, such as `"1-3, 5"` (`None`:
    /// every page).
    pub pages: Option<String>,
}

impl Default for Watermark {
    fn default() -> Self {
        Self {
            text: String::new(),
            picture: None,
            size: 60.0,
            color: "#808080".into(),
            opacity: 0.3,
            angle: 45,
            position: Anchor::Center,
            pages: None,
        }
    }
}

/// How page numbers read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NumberFormat {
    /// `3`
    #[default]
    Plain,
    /// `3 / 12`
    OfTotal,
    /// `Pagina 3`: with the word of [`PageNumbers::word`].
    Word,
}

/// Page numbers drawn on the pages of a new PDF (each document of a split
/// has its own).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, optional_fields = nullable)]
pub struct PageNumbers {
    pub position: Anchor,
    pub format: NumberFormat,
    /// The word before the number (`"Pagina"`, `"Page"`).
    pub word: Option<String>,
    /// Size of the numbers in points.
    pub size: f32,
    /// First page (1-based position) that shows a number.
    pub first: u32,
    /// The number that page shows.
    pub start: u32,
    /// Pages that show no number, such as `"1, 5-6"` (they still count).
    pub skip: Option<String>,
}

impl Default for PageNumbers {
    fn default() -> Self {
        Self {
            position: Anchor::Bottom,
            format: NumberFormat::Plain,
            word: None,
            size: 10.0,
            first: 1,
            start: 1,
            skip: None,
        }
    }
}

/// Choices for PDF documents: pictures to PDF, PDF to pictures, and edited
/// or split PDFs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, optional_fields = nullable)]
pub struct PdfOptions {
    pub page_size: PageSize,
    pub orientation: PageOrientation,
    pub margin: PageMargin,
    /// Several files go into one PDF, in order (otherwise one PDF each).
    pub combine: bool,
    /// PDF to pictures: resolution in dots per inch.
    pub dpi: u32,
    /// PDF to pictures: pages such as `"1-3, 5"` (`None`: every page).
    pub pages: Option<String>,
    /// Word between the name and a page number in file names (`"pagina"`,
    /// `"page"`).
    pub page_label: Option<String>,
    /// Word between the name and a range of pages in file names (`"pagine"`,
    /// `"pages"`).
    pub pages_label: Option<String>,
    /// PDF to PDF, and files combined into one PDF: the pages of the new
    /// document, in order and each with its turn; pages left out are removed
    /// (`None`: every page as it is).
    pub layout: Option<Vec<PdfPage>>,
    /// PDF to PDF: one document, or several.
    pub split: PdfSplit,
    /// Pages in each document when splitting every few pages.
    pub split_every: u32,
    /// Positions (1-based, in the new order) after which a new document starts.
    pub split_after: Vec<u32>,
    /// The new PDFs open only with a password.
    pub protection: Option<PdfProtection>,
    /// Text or a picture drawn over the pages of the new PDFs.
    pub watermark: Option<Watermark>,
    /// Numbers drawn on the pages of the new PDFs.
    pub page_numbers: Option<PageNumbers>,
    /// Pages without text (scans, photos) are read by OCR and get invisible
    /// text, so the new PDFs can be searched.
    pub ocr: bool,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            page_size: PageSize::default(),
            orientation: PageOrientation::default(),
            margin: PageMargin::default(),
            combine: true,
            dpi: 150,
            pages: None,
            page_label: None,
            pages_label: None,
            layout: None,
            split: PdfSplit::default(),
            split_every: 1,
            split_after: Vec::new(),
            protection: None,
            watermark: None,
            page_numbers: None,
            ocr: false,
        }
    }
}

/// How hard a file is compressed ("Leggera", "Forte", "Massima").
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize, TS,
)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CompressLevel {
    /// Hardly visible changes.
    Light,
    /// Smaller, still good for screens and printing at home.
    #[default]
    Strong,
    /// As small as possible while still readable: lower resolution too.
    Max,
}

/// A smaller copy of a file in its own format.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, optional_fields = nullable)]
pub struct CompressOptions {
    pub level: CompressLevel,
    /// Largest acceptable result in bytes: the level gets stronger until the
    /// file fits (`None`: just the level).
    #[ts(type = "number | null")]
    pub max_bytes: Option<u64>,
    /// PDF only: pages may become pictures when that is smaller (their text
    /// can no longer be selected or searched).
    pub pages_as_pictures: bool,
    /// Word after the name of the smaller copy (`"compresso"`, `"compressed"`).
    pub label: Option<String>,
}

/// How a picture is resized (after it is turned and cropped). The sizes are
/// those of the edited picture; "fit" sizes keep the aspect ratio.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "mode", rename_all = "camelCase")]
#[ts(export)]
pub enum Resize {
    /// A percentage of the size (`50`: half as wide and as high).
    #[serde(rename_all = "camelCase")]
    Percent { percent: f32 },
    /// A width and/or a height in pixels. With `keep_ratio`, a missing side
    /// follows the picture's proportions, and both sides make a box the
    /// picture fits in; without, both sides are exact (the proportions may
    /// change) and a missing one stays as it is.
    #[serde(rename_all = "camelCase")]
    Pixels {
        width: Option<u32>,
        height: Option<u32>,
        keep_ratio: bool,
    },
    /// The longest side, in pixels.
    #[serde(rename_all = "camelCase")]
    LongSide { side: u32 },
}

/// The part of a picture that is kept, as fractions (0 to 1) of the picture
/// as it is shown after its turn.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CropRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Changes made to a picture before it is encoded, in this order: turned,
/// mirrored, cropped, resized.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PictureEdit {
    /// Clockwise turn in degrees: 0, 90, 180 or 270.
    pub rotation: u16,
    /// Mirrored left to right, after the turn.
    pub flip_horizontal: bool,
    /// Mirrored top to bottom, after the turn.
    pub flip_vertical: bool,
    #[ts(optional = nullable)]
    pub crop: Option<CropRect>,
    #[ts(optional = nullable)]
    pub resize: Option<Resize>,
    /// The picture may become larger than it is (a drawing always may).
    pub enlarge: bool,
    /// An edited copy in the file's own format ("Modifica"): `target` is
    /// that format, named `name-<label>.ext`.
    pub copy: bool,
    /// Word after the name of the copy (`"mod"`).
    #[ts(optional = nullable)]
    pub label: Option<String>,
}

impl PictureEdit {
    /// Nothing changes the picture.
    pub fn is_empty(&self) -> bool {
        self.rotation.is_multiple_of(360)
            && !self.flip_horizontal
            && !self.flip_vertical
            && self.crop.is_none()
            && self.resize.is_none()
    }
}

/// A track kept in an audio or video file, with its details.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TrackChoice {
    /// Stream index in the file.
    pub index: u32,
    /// ISO 639-2 language code (`None`: none).
    pub language: Option<String>,
    pub title: Option<String>,
    /// Played or shown by default (one per kind of track).
    pub default: bool,
}

/// Subtitle file formats a track can be extracted into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SubtitleFormat {
    Srt,
    Vtt,
    Ass,
}

/// One track saved as a file of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExtractTrack {
    /// Stream index in the file.
    pub index: u32,
    /// Audio: the format (`None`: one that keeps the track as it is).
    #[ts(optional = nullable)]
    pub audio: Option<TargetFormat>,
    /// Subtitles: the file format (`None`: the one closest to the track).
    #[ts(optional = nullable)]
    pub subtitles: Option<SubtitleFormat>,
}

/// Tracks of an audio or video file saved as files of their own: one next
/// to the file (`name-ita.m4a`), several in a folder named after it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ExtractOptions {
    pub tracks: Vec<ExtractTrack>,
}

/// The part of an audio or video file that is kept.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Trim {
    /// Seconds from the start.
    pub start_secs: f64,
    /// Seconds from the start (`None`: the end of the file).
    pub end_secs: Option<f64>,
    /// Cut at the exact moment, encoding again; otherwise the streams are
    /// copied and a video starts at the key frame before `start_secs`.
    pub precise: bool,
}

/// Changes to an audio or video file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MediaEdit {
    /// Tracks kept, in this order (`None`: the first video and audio tracks).
    #[ts(optional = nullable)]
    pub tracks: Option<Vec<TrackChoice>>,
    /// Only a part of the file is kept.
    #[ts(optional = nullable)]
    pub trim: Option<Trim>,
    /// An edited copy in the file's own format ("Modifica"): `target` is
    /// that format, named `name-<label>.ext`; streams are copied when they can be.
    pub copy: bool,
    /// Word after the name of the copy (`"mod"`).
    #[ts(optional = nullable)]
    pub label: Option<String>,
}

/// A conversion requested by the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConversionRequest {
    pub target: TargetFormat,
    #[serde(default)]
    pub preset: QualityPreset,
    #[serde(default)]
    pub advanced: AdvancedOptions,
    /// Only for pictures taken from a video.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub frames: Option<FrameOptions>,
    /// Erase where the photo or video was made (GPS data, location tags).
    #[serde(default)]
    pub remove_location: bool,
    /// Only for PDF documents (pictures to PDF, PDF to pictures).
    #[serde(default)]
    #[ts(optional = nullable)]
    pub pdf: Option<PdfOptions>,
    /// A smaller copy in the same format: `target` is the input's own format,
    /// and the preset and advanced options are chosen by Konvert.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub compress: Option<CompressOptions>,
    /// Pictures only: turned, mirrored, cropped or resized (or an edited
    /// copy in the same format).
    #[serde(default)]
    #[ts(optional = nullable)]
    pub edit: Option<PictureEdit>,
    /// Audio and video only: tracks kept and their details (or an edited
    /// copy in the same format).
    #[serde(default)]
    #[ts(optional = nullable)]
    pub media: Option<MediaEdit>,
    /// Audio and video only: tracks saved as files of their own (`target`
    /// is then the file's own format; the trim of `media` applies).
    #[serde(default)]
    #[ts(optional = nullable)]
    pub extract: Option<ExtractOptions>,
}

impl ConversionRequest {
    /// An edited copy in the file's own format (a picture or a media file).
    pub fn edited_copy(&self) -> Option<&str> {
        match (&self.edit, &self.media) {
            (Some(edit), _) if edit.copy => Some(edit.label.as_deref().unwrap_or("")),
            (_, Some(media)) if media.copy => Some(media.label.as_deref().unwrap_or("")),
            _ => None,
        }
    }

    /// Request with the given preset and no advanced overrides.
    pub fn new(target: TargetFormat, preset: QualityPreset) -> Self {
        Self {
            target,
            preset,
            advanced: AdvancedOptions::default(),
            frames: None,
            remove_location: false,
            pdf: None,
            compress: None,
            edit: None,
            media: None,
            extract: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_json_from_the_ui_is_accepted() {
        let req: ConversionRequest = serde_json::from_str(r#"{ "target": "mp3" }"#).unwrap();
        assert_eq!(
            req,
            ConversionRequest::new(TargetFormat::Mp3, QualityPreset::Balanced)
        );

        let req: ConversionRequest = serde_json::from_str(
            r#"{ "target": "mp4", "preset": "small",
                 "advanced": { "maxHeight": 720, "videoCodec": "av1", "channels": null } }"#,
        )
        .unwrap();
        assert_eq!(req.advanced.max_height, Some(720));
        assert_eq!(req.advanced.video_codec, Some(VideoCodec::Av1));
        assert!(req.advanced.touches_video() && !req.advanced.touches_audio());

        let req: ConversionRequest = serde_json::from_str(
            r#"{ "target": "webp", "advanced": { "imageQuality": 70, "maxDimension": 1920 } }"#,
        )
        .unwrap();
        assert_eq!(req.target, TargetFormat::Webp);
        assert_eq!(
            (req.advanced.image_quality, req.advanced.max_dimension),
            (Some(70), Some(1920))
        );
        assert!(!req.advanced.touches_video() && !req.advanced.touches_audio());
    }

    #[test]
    fn frame_options_are_optional() {
        let req: ConversionRequest = serde_json::from_str(
            r#"{ "target": "png", "frames": { "mode": "sequence", "everySecs": 2 } }"#,
        )
        .unwrap();
        let frames = req.frames.unwrap();
        assert_eq!(
            (frames.mode, frames.at_secs, frames.every_secs),
            (FrameMode::Sequence, None, Some(2.0))
        );
        let req: ConversionRequest =
            serde_json::from_str(r#"{ "target": "jpg", "frames": {} }"#).unwrap();
        assert_eq!(req.frames, Some(FrameOptions::default()));
        assert_eq!(FrameOptions::default().mode, FrameMode::Single);
    }

    #[test]
    fn compress_options_are_optional() {
        let req: ConversionRequest = serde_json::from_str(
            r#"{ "target": "pdf", "compress": { "level": "max", "maxBytes": 5000000 } }"#,
        )
        .unwrap();
        assert_eq!(
            req.compress,
            Some(CompressOptions {
                level: CompressLevel::Max,
                max_bytes: Some(5_000_000),
                pages_as_pictures: false,
                label: None,
            })
        );
        let req: ConversionRequest =
            serde_json::from_str(r#"{ "target": "jpg", "compress": {} }"#).unwrap();
        assert_eq!(req.compress.unwrap().level, CompressLevel::Strong);
        assert!(CompressLevel::Light < CompressLevel::Max);
    }

    #[test]
    fn unknown_formats_are_rejected() {
        assert!(serde_json::from_str::<ConversionRequest>(r#"{ "target": "exe" }"#).is_err());
    }
}
