//! What the user asks for: target format, quality preset and advanced options.
//!
//! These types are the whole contract between the UI and the core: the UI never
//! sends FFmpeg arguments, only a [`ConversionRequest`].

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::formats::{AudioCodec, TargetFormat, VideoCodec};

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

/// Choices for PDF documents: pictures to PDF and PDF to pictures.
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
    /// PDF to pictures: word between the name and the page number in file
    /// names (`"pagina"`, `"page"`).
    pub page_label: Option<String>,
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
        }
    }
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
}

impl ConversionRequest {
    /// Request with the given preset and no advanced overrides.
    pub fn new(target: TargetFormat, preset: QualityPreset) -> Self {
        Self {
            target,
            preset,
            advanced: AdvancedOptions::default(),
            frames: None,
            remove_location: false,
            pdf: None,
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
    fn unknown_formats_are_rejected() {
        assert!(serde_json::from_str::<ConversionRequest>(r#"{ "target": "exe" }"#).is_err());
    }
}
