//! Conversion planning: turns an analysed input and a [`ConversionRequest`] into
//! concrete decisions (copy or re-encode each stream, codecs, quality settings,
//! output size, warnings, size estimate).
//!
//! Pure functions only. The FFmpeg command builder translates a plan into
//! arguments, and the UI shows the plan (warnings, "no re-encoding", estimate).

use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::CoreError;
use crate::estimate::{SizeEstimate, estimate_size};
use crate::formats::{
    self, AudioCodec, FormatKind, FormatSpec, ImageCodec, TargetFormat, VideoCodec,
    is_lossless_audio_codec, is_lossless_image, is_lossless_video_codec,
};
use crate::probe::{AudioStream, MediaInfo, MediaKind, VideoStream};
use crate::request::{ConversionRequest, QualityPreset};

/// How the audio stream is produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "mode", rename_all = "camelCase")]
#[ts(export)]
pub enum AudioPlan {
    /// Copied bit for bit (no quality loss, very fast).
    Copy { codec: String },
    /// Re-encoded.
    Encode(AudioEncode),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioEncode {
    pub codec: AudioCodec,
    pub rate: AudioRate,
    /// Output sample rate (`None`: same as the input).
    pub sample_rate: Option<u32>,
    /// Output channel count (`None`: same as the input).
    pub channels: Option<u32>,
}

/// Rate control of an audio encoder.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum AudioRate {
    /// Encoder VBR quality scale (LAME `-q:a` 0 = best … 9; Vorbis `-q:a` −1 … 10 = best).
    Vbr { quality: i8 },
    /// Target bit rate in kbit/s.
    Bitrate { kbps: u32 },
    /// Uncompressed PCM with the given bit depth.
    Pcm { bits: u32 },
    /// FLAC compression level (0–12; size/speed only, quality is identical) and bit depth.
    Flac { level: u8, bits: u32 },
}

/// How the video stream is produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "mode", rename_all = "camelCase")]
#[ts(export)]
pub enum VideoPlan {
    Copy { codec: String },
    Encode(VideoEncode),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VideoEncode {
    pub codec: VideoCodec,
    pub rate: VideoRate,
    /// Output size (`None` when the input size is unknown).
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// A scale filter is needed (resize or odd dimensions).
    pub resize: bool,
    /// Forced output frame rate (`None`: same as the input).
    pub fps: Option<f64>,
}

/// Rate control of a video encoder.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum VideoRate {
    /// Target bit rate in kbit/s.
    Bitrate { kbps: u32 },
    /// Constant quality (lower = better) and encoder speed preset (higher = faster).
    Crf { crf: u8, speed: u8 },
    /// Fixed quantizer (MPEG-4 `-q:v`, 1 = best … 31).
    Quantizer { q: u8 },
    /// GIF: optimized 256-color palette.
    Palette,
}

/// How a picture is produced. Pictures are always encoded again: copying
/// would just duplicate the input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImagePlan {
    pub codec: ImageCodec,
    pub quality: ImageQuality,
    /// Output size (`None` when the input size is unknown).
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// A scale filter is needed.
    pub resize: bool,
    /// Transparency is kept.
    pub alpha: bool,
    /// Transparent areas are laid on white (the format has no transparency).
    pub flatten: bool,
    /// Tile grid to assemble (HEIC/AVIF photos made of tiles).
    pub grid_group: Option<u32>,
    /// Separate transparency plane to merge back (AVIF).
    pub alpha_stream: Option<u32>,
}

/// Quality setting of a picture encoder.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum ImageQuality {
    /// Encoder quality from 1 (smallest file) to 100 (best).
    Lossy { quality: u8 },
    /// Every pixel is kept; `effort` (PNG 0–9, WebP 0–6) only trades speed for size.
    Lossless { effort: u8 },
}

/// Things the user should know before converting. Ordered by importance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ConversionWarning {
    /// Lossy → lossless (e.g. MP3 → WAV): lost quality is not recovered, the file grows.
    LossyToLossless,
    /// Lossy → lossy re-encoding: some further quality loss.
    LossyReencode,
    /// Lossless → lossy: normal compression with some quality loss.
    LosslessToLossy,
    /// GIF: 256 colors, no audio, large files.
    GifLimitations,
    /// The picture format has no transparency: transparent areas become white.
    TransparencyLost,
    /// The picture is larger than the format allows: it is scaled down.
    PictureDownscaled,
    /// The output format has no audio: the soundtrack is not kept.
    AudioDropped,
    /// The output format cannot store the cover picture.
    CoverArtDropped,
    /// Subtitles are not kept.
    SubtitlesDropped,
    /// Only the first audio track is kept.
    ExtraAudioTracksDropped,
    /// The output format supports at most two channels: downmixed to stereo.
    ChannelsDownmixed,
    /// The photo's EXIF data (camera, date, location) is not kept.
    MetadataDropped,
}

/// Every decision taken for one conversion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConversionPlan {
    pub target: TargetFormat,
    /// Index of the input video stream (or picture) used, if any.
    pub video_stream: Option<u32>,
    pub video: Option<VideoPlan>,
    /// Index of the input audio stream used, if any.
    pub audio_stream: Option<u32>,
    pub audio: Option<AudioPlan>,
    /// Index of the input cover picture to embed, if any.
    pub cover_art_stream: Option<u32>,
    /// Picture output (picture formats only).
    pub image: Option<ImagePlan>,
    pub warnings: Vec<ConversionWarning>,
    pub estimate: Option<SizeEstimate>,
}

impl ConversionPlan {
    /// Nothing is re-encoded (remux only).
    pub fn is_remux(&self) -> bool {
        self.image.is_none()
            && !matches!(self.video, Some(VideoPlan::Encode(_)))
            && !matches!(self.audio, Some(AudioPlan::Encode(_)))
    }
}

/// Plans the conversion of `media` (read from `input`) according to `request`.
pub fn plan_conversion(
    media: &MediaInfo,
    request: &ConversionRequest,
    input: &Path,
) -> Result<ConversionPlan, CoreError> {
    let spec = formats::spec(request.target);
    if !formats::is_offered(media, input, request.target) {
        return Err(unsupported(format!(
            "{} is not offered for this input",
            spec.label
        )));
    }
    let adv = &request.advanced;
    if let Some(codec) = adv.video_codec.filter(|c| !spec.video_codecs.contains(c)) {
        return Err(unsupported(format!(
            "{codec:?} is not available in {}",
            spec.label
        )));
    }
    if let Some(codec) = adv.audio_codec.filter(|c| !spec.audio_codecs.contains(c)) {
        return Err(unsupported(format!(
            "{codec:?} is not available in {}",
            spec.label
        )));
    }
    if spec.kind == FormatKind::Image {
        return plan_picture(media, request, spec);
    }

    // Re-encode when the user asked for a smaller file or for the same format
    // (copying would just duplicate the input).
    let same_format = formats::source_format(input) == Some(request.target);
    let force_encode = request.preset == QualityPreset::Small || same_format;

    let video_stream = match spec.kind {
        FormatKind::Video => Some(
            media
                .primary_video()
                .ok_or_else(|| unsupported("the input has no video".into()))?,
        ),
        FormatKind::Audio | FormatKind::Image => None,
    };
    let video = video_stream.map(|v| plan_video(spec, v, request, force_encode));

    let audio_stream = media
        .primary_audio()
        .filter(|_| !spec.audio_codecs.is_empty());
    if spec.kind == FormatKind::Audio && audio_stream.is_none() {
        return Err(unsupported("the input has no audio track".into()));
    }
    let audio = audio_stream
        .map(|a| plan_audio(spec, a, request, force_encode))
        .transpose()?;

    let cover_art_stream = media
        .cover_art
        .as_ref()
        .filter(|c| spec.cover_art && media.kind == MediaKind::Audio && c.is_portable())
        .map(|c| c.index);

    let mut plan = ConversionPlan {
        target: request.target,
        video_stream: video_stream.map(|v| v.index),
        video,
        audio_stream: audio_stream.map(|a| a.index),
        audio,
        cover_art_stream,
        image: None,
        warnings: Vec::new(),
        estimate: None,
    };
    plan.warnings = collect_warnings(media, request, &plan);
    plan.estimate = estimate_size(media, &plan);
    Ok(plan)
}

fn unsupported(details: String) -> CoreError {
    CoreError::UnsupportedConversion { details }
}

// --- Pictures ------------------------------------------------------------------

fn plan_picture(
    media: &MediaInfo,
    request: &ConversionRequest,
    spec: &FormatSpec,
) -> Result<ConversionPlan, CoreError> {
    use ConversionWarning as W;
    let codec = spec
        .image_codec
        .expect("picture formats have a picture codec");
    let src = media
        .primary_video()
        .ok_or_else(|| unsupported("the input has no picture".into()))?;
    let picture = media.picture.clone().unwrap_or_default();
    let adv = &request.advanced;
    let preset = request.preset;
    let source_lossless = is_lossless_image(&src.codec, src.pix_fmt.as_deref());

    let quality = match (codec, adv.image_quality) {
        (_, Some(q)) if !(1..=100).contains(&q) => {
            return Err(unsupported(format!("picture quality {q} is out of range")));
        }
        (ImageCodec::Png, _) => ImageQuality::Lossless {
            effort: pick(preset, 6, 6, 9),
        },
        // From a lossless source, the best WebP keeps every pixel.
        (ImageCodec::Webp, None) if preset == QualityPreset::Max && source_lossless => {
            ImageQuality::Lossless { effort: 4 }
        }
        (_, Some(quality)) => ImageQuality::Lossy { quality },
        (ImageCodec::Jpeg, None) => ImageQuality::Lossy {
            quality: pick(preset, 95, 85, 70),
        },
        (ImageCodec::Webp, None) => ImageQuality::Lossy {
            quality: pick(preset, 95, 82, 65),
        },
        // Same scale as libavif's quality (60 is its default).
        (ImageCodec::Avif, None) => ImageQuality::Lossy {
            quality: pick(preset, 75, 60, 45),
        },
    };

    let wanted = adv.max_dimension.filter(|d| *d > 0);
    let limit = [wanted, codec.max_dimension()].into_iter().flatten().min();
    let source_size = src.width.zip(src.height).filter(|(w, h)| *w > 0 && *h > 0);
    let size = source_size.map(|(w, h)| fit_within(w, h, limit));
    let resize = size.is_some() && size != source_size;

    let image = ImagePlan {
        codec,
        quality,
        width: size.map(|s| s.0),
        height: size.map(|s| s.1),
        resize,
        alpha: picture.alpha && codec.supports_alpha(),
        flatten: picture.alpha && !codec.supports_alpha(),
        grid_group: picture.grid_group,
        alpha_stream: picture.alpha_stream,
    };

    let mut warnings = Vec::new();
    let lossless = matches!(quality, ImageQuality::Lossless { .. });
    match (source_lossless, lossless) {
        (false, true) => warnings.push(W::LossyToLossless),
        (false, false) => warnings.push(W::LossyReencode),
        (true, false) => warnings.push(W::LosslessToLossy),
        (true, true) => {}
    }
    if image.flatten {
        warnings.push(W::TransparencyLost);
    }
    let longest = source_size.map_or(0, |(w, h)| w.max(h));
    let format_limit = codec.max_dimension().filter(|max| longest > *max);
    if format_limit.is_some_and(|max| wanted.is_none_or(|w| w > max)) {
        warnings.push(W::PictureDownscaled);
    }
    // FFmpeg carries EXIF data over only from JPEG/PNG/WebP files into PNG.
    if picture.exif && (codec != ImageCodec::Png || media.is_heif()) {
        warnings.push(W::MetadataDropped);
    }
    warnings.sort_by_key(|w| *w as u8);

    let mut plan = ConversionPlan {
        target: request.target,
        video_stream: Some(src.index),
        video: None,
        audio_stream: None,
        audio: None,
        cover_art_stream: None,
        image: Some(image),
        warnings,
        estimate: None,
    };
    plan.estimate = estimate_size(media, &plan);
    Ok(plan)
}

/// Scales `width`×`height` so that the longest side is at most `limit`
/// (never upscales, keeps the aspect ratio).
fn fit_within(width: u32, height: u32, limit: Option<u32>) -> (u32, u32) {
    match limit {
        Some(max) if width.max(height) > max => {
            if width >= height {
                (max, scaled(height, max, width).max(1))
            } else {
                (scaled(width, max, height).max(1), max)
            }
        }
        _ => (width, height),
    }
}

// --- Audio ---------------------------------------------------------------------

fn plan_audio(
    spec: &FormatSpec,
    src: &AudioStream,
    request: &ConversionRequest,
    force_encode: bool,
) -> Result<AudioPlan, CoreError> {
    let adv = &request.advanced;
    if !force_encode && !adv.touches_audio() && spec.copy_audio.contains(&src.codec.as_str()) {
        return Ok(AudioPlan::Copy {
            codec: src.codec.clone(),
        });
    }
    let codec = adv
        .audio_codec
        .or(spec.default_audio_codec())
        .expect("audio formats have an audio codec");

    let channels =
        adv.channels
            .map(|c| c.count())
            .or_else(|| match (codec.max_channels(), src.channels) {
                (Some(max), Some(n)) if n > max => Some(max),
                _ => None,
            });
    let sample_rate = match (
        adv.sample_rate,
        codec.supported_sample_rates(),
        src.sample_rate,
    ) {
        (Some(rate), Some(allowed), _) if !allowed.contains(&rate) => {
            return Err(unsupported(format!(
                "{rate} Hz is not supported by {codec:?}"
            )));
        }
        (Some(rate), _, _) => Some(rate),
        (None, Some(allowed), Some(src_rate)) if !allowed.contains(&src_rate) => {
            Some(nearest_supported_rate(allowed, src_rate))
        }
        _ => None,
    };

    let out_channels = channels.or(src.channels).unwrap_or(2);
    let preset = request.preset;
    let bitrate = |max: u32, balanced: u32, small: u32, cap: u32| AudioRate::Bitrate {
        kbps: adv.audio_bitrate_kbps.unwrap_or_else(|| {
            let stereo = pick(preset, max, balanced, small);
            (stereo * out_channels / 2).clamp(32, cap)
        }),
    };
    let rate = match codec {
        AudioCodec::Mp3 => match adv.audio_bitrate_kbps {
            Some(kbps) => AudioRate::Bitrate { kbps },
            // VBR MP3 inside video containers (AVI) can drift out of sync: use CBR.
            None if spec.kind == FormatKind::Video => bitrate(256, 192, 128, 320),
            None => AudioRate::Vbr {
                quality: pick(preset, 0, 2, 5),
            },
        },
        AudioCodec::Vorbis => match adv.audio_bitrate_kbps {
            Some(kbps) => AudioRate::Bitrate { kbps },
            None => AudioRate::Vbr {
                quality: pick(preset, 8, 6, 3),
            },
        },
        AudioCodec::Aac => bitrate(256, 192, 128, 512),
        AudioCodec::Opus => bitrate(192, 128, 64, 510),
        AudioCodec::Wma => bitrate(192, 160, 96, 192),
        AudioCodec::Pcm => AudioRate::Pcm {
            bits: lossless_bit_depth(src, preset),
        },
        AudioCodec::Flac => AudioRate::Flac {
            level: pick(preset, 5, 5, 8),
            bits: lossless_bit_depth(src, preset),
        },
    };
    Ok(AudioPlan::Encode(AudioEncode {
        codec,
        rate,
        sample_rate,
        channels,
    }))
}

/// 24-bit when the source has more than 16 bits (except "small file"), else 16-bit.
fn lossless_bit_depth(src: &AudioStream, preset: QualityPreset) -> u32 {
    let high_res = is_lossless_audio_codec(&src.codec) && src.bit_depth.is_some_and(|b| b > 16);
    if high_res && preset != QualityPreset::Small {
        24
    } else {
        16
    }
}

/// Smallest supported rate ≥ `rate`, or the highest supported one.
fn nearest_supported_rate(allowed: &[u32], rate: u32) -> u32 {
    allowed
        .iter()
        .copied()
        .filter(|r| *r >= rate)
        .min()
        .unwrap_or_else(|| allowed.iter().copied().max().unwrap_or(rate))
}

fn pick<T>(preset: QualityPreset, max: T, balanced: T, small: T) -> T {
    match preset {
        QualityPreset::Max => max,
        QualityPreset::Balanced => balanced,
        QualityPreset::Small => small,
    }
}

// --- Video ---------------------------------------------------------------------

fn plan_video(
    spec: &FormatSpec,
    src: &VideoStream,
    request: &ConversionRequest,
    force_encode: bool,
) -> VideoPlan {
    let adv = &request.advanced;
    if !force_encode && !adv.touches_video() && spec.copy_video.contains(&src.codec.as_str()) {
        return VideoPlan::Copy {
            codec: src.codec.clone(),
        };
    }
    let codec = adv
        .video_codec
        .or(spec.default_video_codec())
        .expect("video formats have a video codec");
    let preset = request.preset;

    let size = match (src.width, src.height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => {
            Some(output_size(codec, w, h, adv.max_height, preset))
        }
        _ => None,
    };
    let resize = size.is_some_and(|s| Some(s) != src.width.zip(src.height));

    let fps = match codec {
        VideoCodec::Gif => {
            let wanted = adv.fps.unwrap_or(pick(preset, 24.0, 15.0, 10.0));
            Some(src.fps.map_or(wanted, |f| wanted.min(f)))
        }
        _ => adv.fps,
    };

    let rate = match (codec, adv.video_bitrate_kbps) {
        (VideoCodec::Gif, _) => VideoRate::Palette,
        (_, Some(kbps)) => VideoRate::Bitrate { kbps },
        (VideoCodec::H264, None) => VideoRate::Bitrate {
            kbps: h264_bitrate_kbps(src, size, fps, preset),
        },
        (VideoCodec::Vp9, None) => VideoRate::Crf {
            crf: pick(preset, 24, 31, 37),
            speed: pick(preset, 2, 3, 4),
        },
        (VideoCodec::Av1, None) => VideoRate::Crf {
            crf: pick(preset, 26, 34, 42),
            speed: pick(preset, 6, 8, 10),
        },
        (VideoCodec::Mpeg4, None) => VideoRate::Quantizer {
            q: pick(preset, 2, 4, 7),
        },
    };

    VideoPlan::Encode(VideoEncode {
        codec,
        rate,
        width: size.map(|s| s.0),
        height: size.map(|s| s.1),
        resize,
        fps,
    })
}

/// Output dimensions: never upscales, keeps the aspect ratio. YUV 4:2:0 codecs
/// need even dimensions; GIF is limited to a preset width.
fn output_size(
    codec: VideoCodec,
    width: u32,
    height: u32,
    max_height: Option<u32>,
    preset: QualityPreset,
) -> (u32, u32) {
    let scale_to_height = |h: u32| (scaled(width, h, height), h);
    let (w, h) = match (codec, max_height) {
        (_, Some(max_h)) if max_h < height => scale_to_height(max_h),
        (VideoCodec::Gif, None) => {
            let max_w = pick(preset, 960, 640, 480);
            if width > max_w {
                (max_w, scaled(height, max_w, width))
            } else {
                (width, height)
            }
        }
        _ => (width, height),
    };
    if codec == VideoCodec::Gif {
        (w.max(1), h.max(1))
    } else {
        (even(w), even(h))
    }
}

/// `value * num / den`, rounded.
fn scaled(value: u32, num: u32, den: u32) -> u32 {
    ((value as u64 * num as u64 + den as u64 / 2) / den as u64) as u32
}

fn even(v: u32) -> u32 {
    (v & !1).max(2)
}

/// H.264 (OpenH264 has no constant-quality mode) bit rate from resolution and
/// frame rate, capped by the source bit rate so files do not grow needlessly.
fn h264_bitrate_kbps(
    src: &VideoStream,
    size: Option<(u32, u32)>,
    fps: Option<f64>,
    preset: QualityPreset,
) -> u32 {
    let (w, h) = size.unwrap_or((1280, 720));
    let fps = fps.or(src.fps).unwrap_or(30.0).clamp(1.0, 60.0);
    let bits_per_pixel = pick(preset, 0.10, 0.07, 0.045);
    let mut kbps = w as f64 * h as f64 * fps * bits_per_pixel / 1000.0;

    if let (Some(src_bps), Some(src_w), Some(src_h)) = (src.bit_rate, src.width, src.height) {
        let area_ratio = (w as f64 * h as f64) / (src_w as f64 * src_h as f64).max(1.0);
        let efficiency = codec_efficiency(&src.codec);
        let cap = src_bps as f64 / 1000.0 * efficiency * area_ratio * pick(preset, 1.25, 1.0, 0.6);
        kbps = kbps.min(cap);
    }
    ((kbps / 10.0).round() as u32 * 10).max(150)
}

/// Compression efficiency relative to H.264 (rough, for bit rate caps).
fn codec_efficiency(codec: &str) -> f64 {
    match codec {
        "hevc" => 1.5,
        "vp9" => 1.4,
        "av1" => 1.8,
        "mpeg4" | "msmpeg4v3" | "wmv2" | "wmv3" => 0.7,
        "mpeg2video" | "mpeg1video" | "mjpeg" => 0.5,
        _ => 1.0,
    }
}

// --- Warnings --------------------------------------------------------------------

fn collect_warnings(
    media: &MediaInfo,
    request: &ConversionRequest,
    plan: &ConversionPlan,
) -> Vec<ConversionWarning> {
    use ConversionWarning as W;
    let mut warnings = Vec::new();

    if let (Some(AudioPlan::Encode(enc)), Some(src)) = (&plan.audio, media.primary_audio()) {
        match (is_lossless_audio_codec(&src.codec), enc.codec.is_lossless()) {
            (false, true) => warnings.push(W::LossyToLossless),
            (false, false) => warnings.push(W::LossyReencode),
            (true, false) => warnings.push(W::LosslessToLossy),
            (true, true) => {}
        }
        let downmixed = request.advanced.channels.is_none()
            && enc
                .channels
                .zip(src.channels)
                .is_some_and(|(out, inp)| out < inp);
        if downmixed {
            warnings.push(W::ChannelsDownmixed);
        }
    }
    if let (Some(VideoPlan::Encode(enc)), Some(src)) = (&plan.video, media.primary_video())
        && enc.codec != VideoCodec::Gif
        && !is_lossless_video_codec(&src.codec)
    {
        warnings.push(W::LossyReencode);
    }
    if plan.target == TargetFormat::Gif {
        warnings.push(W::GifLimitations);
        if !media.audio.is_empty() {
            warnings.push(W::AudioDropped);
        }
    }
    if media.kind == MediaKind::Audio
        && media.cover_art.is_some()
        && plan.cover_art_stream.is_none()
    {
        warnings.push(W::CoverArtDropped);
    }
    if media.subtitle_streams > 0 {
        warnings.push(W::SubtitlesDropped);
    }
    if media.audio.len() > 1 && plan.audio.is_some() {
        warnings.push(W::ExtraAudioTracksDropped);
    }

    // Keep the first occurrence of each warning, then order by importance.
    let mut unique = Vec::new();
    for w in warnings {
        if !unique.contains(&w) {
            unique.push(w);
        }
    }
    unique.sort_by_key(|w| *w as u8);
    unique
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::parse_ffprobe_json;
    use crate::request::{AdvancedOptions, ChannelLayout};

    use ConversionWarning as W;

    fn media(json: &str) -> MediaInfo {
        parse_ffprobe_json(json.as_bytes()).unwrap()
    }

    fn wav_24bit() -> MediaInfo {
        media(
            r#"{ "streams": [{ "index": 0, "codec_name": "pcm_s24le", "codec_type": "audio",
            "sample_rate": "96000", "channels": 2, "bits_per_sample": 24 }],
            "format": { "format_name": "wav", "duration": "60" } }"#,
        )
    }

    fn mp3_with_cover() -> MediaInfo {
        media(
            r#"{ "streams": [
            { "index": 0, "codec_name": "mp3", "codec_type": "audio", "sample_rate": "44100",
              "channels": 2, "bit_rate": "320000" },
            { "index": 1, "codec_name": "mjpeg", "codec_type": "video", "disposition": { "attached_pic": 1 } }],
            "format": { "format_name": "mp3", "duration": "200" } }"#,
        )
    }

    fn mp4_1080p() -> MediaInfo {
        media(
            r#"{ "streams": [
            { "index": 0, "codec_name": "h264", "codec_type": "video", "width": 1920, "height": 1080,
              "avg_frame_rate": "30/1", "bit_rate": "8000000" },
            { "index": 1, "codec_name": "aac", "codec_type": "audio", "sample_rate": "48000",
              "channels": 6, "bit_rate": "384000" },
            { "index": 2, "codec_name": "aac", "codec_type": "audio", "sample_rate": "48000", "channels": 2 },
            { "index": 3, "codec_name": "mov_text", "codec_type": "subtitle" }],
            "format": { "format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "120" } }"#,
        )
    }

    fn req(target: TargetFormat, preset: QualityPreset) -> ConversionRequest {
        ConversionRequest::new(target, preset)
    }

    fn plan(media: &MediaInfo, request: &ConversionRequest, input: &str) -> ConversionPlan {
        plan_conversion(media, request, Path::new(input)).unwrap()
    }

    fn audio_encode(p: &ConversionPlan) -> &AudioEncode {
        match &p.audio {
            Some(AudioPlan::Encode(e)) => e,
            other => panic!("expected audio encode, got {other:?}"),
        }
    }

    fn video_encode(p: &ConversionPlan) -> &VideoEncode {
        match &p.video {
            Some(VideoPlan::Encode(e)) => e,
            other => panic!("expected video encode, got {other:?}"),
        }
    }

    #[test]
    fn wav_to_mp3_uses_vbr_and_resamples_96k() {
        let p = plan(
            &wav_24bit(),
            &req(TargetFormat::Mp3, QualityPreset::Balanced),
            "a.wav",
        );
        let enc = audio_encode(&p);
        assert_eq!(enc.codec, AudioCodec::Mp3);
        assert_eq!(enc.rate, AudioRate::Vbr { quality: 2 });
        assert_eq!(enc.sample_rate, Some(48000), "LAME has no 96 kHz");
        assert_eq!(p.warnings, [W::LosslessToLossy]);
    }

    #[test]
    fn mp3_to_wav_warns_that_quality_is_not_recovered() {
        let p = plan(
            &mp3_with_cover(),
            &req(TargetFormat::Wav, QualityPreset::Max),
            "a.mp3",
        );
        assert_eq!(audio_encode(&p).rate, AudioRate::Pcm { bits: 16 });
        assert_eq!(p.warnings, [W::LossyToLossless, W::CoverArtDropped]);
    }

    #[test]
    fn high_resolution_audio_keeps_24_bit_except_small_preset() {
        let src = wav_24bit();
        let max = plan(&src, &req(TargetFormat::Flac, QualityPreset::Max), "a.wav");
        assert_eq!(
            audio_encode(&max).rate,
            AudioRate::Flac { level: 5, bits: 24 }
        );
        let small = plan(
            &src,
            &req(TargetFormat::Flac, QualityPreset::Small),
            "a.wav",
        );
        assert_eq!(
            audio_encode(&small).rate,
            AudioRate::Flac { level: 8, bits: 16 }
        );
        assert!(small.warnings.is_empty());
    }

    #[test]
    fn cover_art_is_kept_where_supported() {
        let src = mp3_with_cover();
        let flac = plan(
            &src,
            &req(TargetFormat::Flac, QualityPreset::Balanced),
            "a.mp3",
        );
        assert_eq!(flac.cover_art_stream, Some(1));
        let m4a = plan(
            &src,
            &req(TargetFormat::M4a, QualityPreset::Balanced),
            "a.mp3",
        );
        assert_eq!(m4a.cover_art_stream, Some(1));
        assert_eq!(m4a.warnings, [W::LossyReencode]);
        let ogg = plan(
            &src,
            &req(TargetFormat::Ogg, QualityPreset::Balanced),
            "a.mp3",
        );
        assert_eq!(ogg.cover_art_stream, None);
        assert!(ogg.warnings.contains(&W::CoverArtDropped));
    }

    #[test]
    fn same_lossy_format_is_reencoded_never_copied() {
        let p = plan(
            &mp3_with_cover(),
            &req(TargetFormat::Mp3, QualityPreset::Max),
            "a.mp3",
        );
        assert_eq!(audio_encode(&p).rate, AudioRate::Vbr { quality: 0 });
        assert_eq!(p.cover_art_stream, Some(1));
    }

    #[test]
    fn compatible_streams_are_copied_without_reencoding() {
        let src = mp4_1080p();
        let mkv = plan(
            &src,
            &req(TargetFormat::Mkv, QualityPreset::Balanced),
            "clip.mp4",
        );
        assert_eq!(
            mkv.video,
            Some(VideoPlan::Copy {
                codec: "h264".into()
            })
        );
        assert_eq!(
            mkv.audio,
            Some(AudioPlan::Copy {
                codec: "aac".into()
            })
        );
        assert!(mkv.is_remux());
        assert_eq!(
            mkv.warnings,
            [W::SubtitlesDropped, W::ExtraAudioTracksDropped]
        );
        assert_eq!((mkv.video_stream, mkv.audio_stream), (Some(0), Some(1)));

        // Extracting AAC into M4A is a copy too.
        let m4a = plan(
            &src,
            &req(TargetFormat::M4a, QualityPreset::Max),
            "clip.mp4",
        );
        assert_eq!(
            m4a.audio,
            Some(AudioPlan::Copy {
                codec: "aac".into()
            })
        );
        assert_eq!(m4a.video, None);
    }

    #[test]
    fn small_preset_reencodes_even_compatible_streams() {
        let p = plan(
            &mp4_1080p(),
            &req(TargetFormat::Mkv, QualityPreset::Small),
            "clip.mp4",
        );
        let video = video_encode(&p);
        assert_eq!(video.codec, VideoCodec::H264);
        // 1920×1080 × 30 fps × 0.045 bpp ≈ 2800 kbit/s, below the source cap (8000 × 0.6).
        assert_eq!(video.rate, VideoRate::Bitrate { kbps: 2800 });
        assert!(!video.resize);
        assert!(p.warnings.contains(&W::LossyReencode));
    }

    #[test]
    fn h264_bitrate_is_capped_by_a_low_bitrate_source() {
        let src = media(
            r#"{ "streams": [{ "index": 0, "codec_name": "h264", "codec_type": "video",
            "width": 1920, "height": 1080, "avg_frame_rate": "30/1", "bit_rate": "1500000" }],
            "format": { "format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "10" } }"#,
        );
        let p = plan(
            &src,
            &req(TargetFormat::Mp4, QualityPreset::Balanced),
            "clip.mp4",
        );
        assert_eq!(video_encode(&p).rate, VideoRate::Bitrate { kbps: 1500 });
    }

    #[test]
    fn webm_uses_vp9_crf_and_opus() {
        let p = plan(
            &mp4_1080p(),
            &req(TargetFormat::Webm, QualityPreset::Balanced),
            "clip.mp4",
        );
        assert_eq!(video_encode(&p).rate, VideoRate::Crf { crf: 31, speed: 3 });
        let audio = audio_encode(&p);
        assert_eq!(audio.codec, AudioCodec::Opus);
        // 5.1 source: 128 kbit/s stereo scaled to six channels.
        assert_eq!(audio.rate, AudioRate::Bitrate { kbps: 384 });
    }

    #[test]
    fn mp3_downmixes_surround_and_warns() {
        let p = plan(
            &mp4_1080p(),
            &req(TargetFormat::Mp3, QualityPreset::Balanced),
            "clip.mp4",
        );
        assert_eq!(audio_encode(&p).channels, Some(2));
        assert!(p.warnings.contains(&W::ChannelsDownmixed));
        assert_eq!(p.video, None);
    }

    #[test]
    fn advanced_options_override_the_preset() {
        let mut request = req(TargetFormat::Mp4, QualityPreset::Balanced);
        request.advanced = AdvancedOptions {
            max_height: Some(720),
            fps: Some(25.0),
            channels: Some(ChannelLayout::Stereo),
            audio_bitrate_kbps: Some(160),
            sample_rate: Some(44100),
            ..Default::default()
        };
        let p = plan(&mp4_1080p(), &request, "clip.mp4");
        let video = video_encode(&p);
        assert_eq!(
            (video.width, video.height, video.resize),
            (Some(1280), Some(720), true)
        );
        assert_eq!(video.fps, Some(25.0));
        let audio = audio_encode(&p);
        assert_eq!(audio.rate, AudioRate::Bitrate { kbps: 160 });
        assert_eq!((audio.channels, audio.sample_rate), (Some(2), Some(44100)));
        // The user chose stereo: no downmix warning.
        assert!(!p.warnings.contains(&W::ChannelsDownmixed));
    }

    #[test]
    fn never_upscales_and_fixes_odd_dimensions() {
        let src = media(
            r#"{ "streams": [{ "index": 0, "codec_name": "h264", "codec_type": "video",
            "width": 853, "height": 481 }], "format": { "format_name": "matroska,webm" } }"#,
        );
        let mut request = req(TargetFormat::Mp4, QualityPreset::Max);
        request.advanced.max_height = Some(1080);
        let video = video_encode(&plan(&src, &request, "clip.mkv")).clone();
        assert_eq!(
            (video.width, video.height, video.resize),
            (Some(852), Some(480), true)
        );
    }

    #[test]
    fn gif_limits_width_and_fps_and_drops_audio() {
        let p = plan(
            &mp4_1080p(),
            &req(TargetFormat::Gif, QualityPreset::Balanced),
            "clip.mp4",
        );
        let video = video_encode(&p);
        assert_eq!(video.rate, VideoRate::Palette);
        assert_eq!((video.width, video.height), (Some(640), Some(360)));
        assert_eq!(video.fps, Some(15.0));
        assert_eq!(p.audio, None);
        assert!(
            p.warnings
                .starts_with(&[W::GifLimitations, W::AudioDropped])
        );
    }

    #[test]
    fn invalid_choices_are_rejected() {
        let src = mp4_1080p();
        let mut request = req(TargetFormat::Webm, QualityPreset::Balanced);
        request.advanced.video_codec = Some(VideoCodec::H264);
        assert_eq!(
            plan_conversion(&src, &request, Path::new("clip.mp4"))
                .unwrap_err()
                .code(),
            "unsupported_conversion"
        );

        let mut request = req(TargetFormat::Opus, QualityPreset::Balanced);
        request.advanced.sample_rate = Some(44100);
        assert!(plan_conversion(&src, &request, Path::new("clip.mp4")).is_err());

        // Audio → video is never offered.
        let err = plan_conversion(
            &wav_24bit(),
            &req(TargetFormat::Mp4, QualityPreset::Max),
            Path::new("a.wav"),
        );
        assert_eq!(err.unwrap_err().code(), "unsupported_conversion");
    }

    /// A still picture as `probe` returns it (first frame applied).
    fn picture(
        codec: &str,
        container: &str,
        size: (u32, u32),
        pix_fmt: &str,
        exif: bool,
    ) -> MediaInfo {
        let mut info = media(&format!(
            r#"{{ "streams": [{{ "index": 0, "codec_name": "{codec}", "codec_type": "video",
            "width": {}, "height": {}, "pix_fmt": "{pix_fmt}" }}],
            "format": {{ "format_name": "{container}", "size": "3000000" }} }}"#,
            size.0, size.1
        ));
        let side_data = if exif {
            r#"[{ "side_data_type": "EXIF metadata" }]"#
        } else {
            "[]"
        };
        let frame = format!(
            r#"{{ "frames": [{{ "width": {}, "height": {}, "pix_fmt": "{pix_fmt}",
            "side_data_list": {side_data} }}] }}"#,
            size.0, size.1
        );
        crate::probe::apply_first_frame_json(&mut info, frame.as_bytes()).unwrap();
        info
    }

    fn photo() -> MediaInfo {
        picture("mjpeg", "image2", (4032, 3024), "yuvj420p", true)
    }

    fn transparent_png() -> MediaInfo {
        picture("png", "png_pipe", (800, 600), "rgba", false)
    }

    fn image_plan(p: &ConversionPlan) -> &ImagePlan {
        p.image.as_ref().expect("picture plan")
    }

    #[test]
    fn photo_to_webp_is_reencoded_at_the_preset_quality() {
        let p = plan(
            &photo(),
            &req(TargetFormat::Webp, QualityPreset::Balanced),
            "IMG_0001.JPG",
        );
        let image = image_plan(&p);
        assert_eq!(image.codec, ImageCodec::Webp);
        assert_eq!(image.quality, ImageQuality::Lossy { quality: 82 });
        assert_eq!(
            (image.width, image.height, image.resize),
            (Some(4032), Some(3024), false)
        );
        assert!(!image.alpha && !image.flatten);
        assert_eq!(
            (p.video_stream, &p.video, &p.audio),
            (Some(0), &None, &None)
        );
        assert!(!p.is_remux());
        assert_eq!(p.warnings, [W::LossyReencode, W::MetadataDropped]);
        assert!(p.estimate.unwrap().approximate);

        let quality =
            |target, preset| match image_plan(&plan(&photo(), &req(target, preset), "a.jpg"))
                .quality
            {
                ImageQuality::Lossy { quality } => quality,
                other => panic!("{other:?}"),
            };
        assert_eq!(quality(TargetFormat::Jpg, QualityPreset::Max), 95);
        assert_eq!(quality(TargetFormat::Jpg, QualityPreset::Small), 70);
        assert_eq!(quality(TargetFormat::Avif, QualityPreset::Balanced), 60);
    }

    #[test]
    fn exif_data_is_kept_only_in_png_from_classic_formats() {
        let png = plan(
            &photo(),
            &req(TargetFormat::Png, QualityPreset::Max),
            "a.jpg",
        );
        assert_eq!(png.warnings, [W::LossyToLossless]);
        assert_eq!(
            image_plan(&png).quality,
            ImageQuality::Lossless { effort: 6 }
        );

        // HEIC: FFmpeg reads the EXIF data but cannot carry it over.
        let heic = media(
            r#"{ "streams": [{ "index": 0, "codec_name": "hevc", "codec_type": "video",
            "width": 512, "height": 384, "pix_fmt": "yuv420p",
            "side_data_list": [{ "side_data_type": "EXIF metadata" }] }],
            "format": { "format_name": "mov,mp4,m4a,3gp,3g2,mj2", "tags": { "major_brand": "heic" } } }"#,
        );
        let p = plan(
            &heic,
            &req(TargetFormat::Png, QualityPreset::Small),
            "a.heic",
        );
        assert!(p.warnings.contains(&W::MetadataDropped));
        assert_eq!(image_plan(&p).quality, ImageQuality::Lossless { effort: 9 });
    }

    #[test]
    fn transparency_is_kept_or_laid_on_white() {
        let jpg = plan(
            &transparent_png(),
            &req(TargetFormat::Jpg, QualityPreset::Balanced),
            "logo.png",
        );
        let image = image_plan(&jpg);
        assert!(image.flatten && !image.alpha);
        assert_eq!(jpg.warnings, [W::LosslessToLossy, W::TransparencyLost]);

        for target in [TargetFormat::Webp, TargetFormat::Avif] {
            let p = plan(
                &transparent_png(),
                &req(target, QualityPreset::Balanced),
                "logo.png",
            );
            assert!(
                image_plan(&p).alpha && !image_plan(&p).flatten,
                "{target:?}"
            );
            assert_eq!(p.warnings, [W::LosslessToLossy], "{target:?}");
        }
    }

    #[test]
    fn best_webp_of_a_lossless_picture_is_lossless() {
        let max = plan(
            &transparent_png(),
            &req(TargetFormat::Webp, QualityPreset::Max),
            "a.png",
        );
        assert_eq!(
            image_plan(&max).quality,
            ImageQuality::Lossless { effort: 4 }
        );
        assert!(max.warnings.is_empty());
        // From a JPEG, lossless would only make the file bigger.
        let from_jpeg = plan(
            &photo(),
            &req(TargetFormat::Webp, QualityPreset::Max),
            "a.jpg",
        );
        assert_eq!(
            image_plan(&from_jpeg).quality,
            ImageQuality::Lossy { quality: 95 }
        );
    }

    #[test]
    fn longest_side_is_limited_without_upscaling() {
        let mut request = req(TargetFormat::Jpg, QualityPreset::Balanced);
        request.advanced.max_dimension = Some(1920);
        let image = image_plan(&plan(&photo(), &request, "a.jpg")).clone();
        assert_eq!(
            (image.width, image.height, image.resize),
            (Some(1920), Some(1440), true)
        );

        let portrait = picture("mjpeg", "image2", (3024, 4032), "yuvj420p", false);
        let image = image_plan(&plan(&portrait, &request, "a.jpg")).clone();
        assert_eq!((image.width, image.height), (Some(1440), Some(1920)));

        request.advanced.max_dimension = Some(8000);
        let image = image_plan(&plan(&photo(), &request, "a.jpg")).clone();
        assert_eq!(
            (image.width, image.height, image.resize),
            (Some(4032), Some(3024), false)
        );
    }

    #[test]
    fn pictures_beyond_the_format_limit_are_scaled_down() {
        let panorama = picture("mjpeg", "image2", (20000, 5000), "yuvj420p", false);
        let p = plan(
            &panorama,
            &req(TargetFormat::Webp, QualityPreset::Balanced),
            "pano.jpg",
        );
        let image = image_plan(&p);
        assert_eq!(
            (image.width, image.height, image.resize),
            (Some(16383), Some(4096), true)
        );
        assert!(p.warnings.contains(&W::PictureDownscaled));

        // The user already asked for a smaller picture: nothing to tell.
        let mut request = req(TargetFormat::Webp, QualityPreset::Balanced);
        request.advanced.max_dimension = Some(4000);
        let p = plan(&panorama, &request, "pano.jpg");
        assert!(!p.warnings.contains(&W::PictureDownscaled));
        // JPEG has room for it.
        let p = plan(
            &panorama,
            &req(TargetFormat::Jpg, QualityPreset::Balanced),
            "pano.jpg",
        );
        assert!(!image_plan(&p).resize);
    }

    #[test]
    fn picture_quality_override_is_validated() {
        let mut request = req(TargetFormat::Avif, QualityPreset::Small);
        request.advanced.image_quality = Some(80);
        assert_eq!(
            image_plan(&plan(&photo(), &request, "a.jpg")).quality,
            ImageQuality::Lossy { quality: 80 }
        );
        for wrong in [0, 101] {
            request.advanced.image_quality = Some(wrong);
            let err = plan_conversion(&photo(), &request, Path::new("a.jpg")).unwrap_err();
            assert_eq!(err.code(), "unsupported_conversion");
        }
        // Video options make no sense for a picture.
        let mut request = req(TargetFormat::Png, QualityPreset::Balanced);
        request.advanced.video_codec = Some(VideoCodec::H264);
        assert!(plan_conversion(&photo(), &request, Path::new("a.jpg")).is_err());
    }

    #[test]
    fn opus_resamples_cd_audio_to_48k() {
        let src = media(
            r#"{ "streams": [{ "index": 0, "codec_name": "flac", "codec_type": "audio",
            "sample_rate": "44100", "channels": 1 }], "format": { "format_name": "flac" } }"#,
        );
        let p = plan(
            &src,
            &req(TargetFormat::Opus, QualityPreset::Small),
            "a.flac",
        );
        let enc = audio_encode(&p);
        assert_eq!(enc.sample_rate, Some(48000));
        // Mono: half of the stereo bit rate, but never below 32 kbit/s.
        assert_eq!(enc.rate, AudioRate::Bitrate { kbps: 32 });
    }
}
