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
}

impl ConversionRequest {
    /// Request with the given preset and no advanced overrides.
    pub fn new(target: TargetFormat, preset: QualityPreset) -> Self {
        Self {
            target,
            preset,
            advanced: AdvancedOptions::default(),
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
    fn unknown_formats_are_rejected() {
        assert!(serde_json::from_str::<ConversionRequest>(r#"{ "target": "exe" }"#).is_err());
    }
}
