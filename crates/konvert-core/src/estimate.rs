//! Estimated size of the converted file.
//!
//! Exact for constant bit rates and PCM, approximate for VBR, constant-quality
//! and lossless compression (the UI shows those as "circa").

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::formats::{AudioCodec, ImageCodec, VideoCodec, is_lossless_image};
use crate::plan::{
    AudioPlan, AudioRate, ConversionPlan, ImagePlan, ImageQuality, VideoPlan, VideoRate,
};
use crate::probe::{MediaInfo, MediaKind};
use crate::request::FrameMode;

/// Estimated output size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SizeEstimate {
    pub bytes: u64,
    /// The real size may differ noticeably (VBR, constant quality, FLAC, GIF).
    pub approximate: bool,
}

/// Container overhead (headers, indexes, padding).
const OVERHEAD: f64 = 1.01;

/// Estimates the output size; `None` when the duration (or, for a picture,
/// the size) is unknown.
pub fn estimate_size(media: &MediaInfo, plan: &ConversionPlan) -> Option<SizeEstimate> {
    if let Some(image) = &plan.image {
        let picture = estimate_picture(media, image)?;
        return match &plan.frames {
            Some(frames) if frames.mode == FrameMode::Sequence => Some(SizeEstimate {
                bytes: picture.bytes * frames.count? as u64,
                approximate: true,
            }),
            _ => Some(picture),
        };
    }
    let duration = plan
        .trim
        .map(|(_, length)| length)
        .or(media.duration_secs)?;
    let mut approximate = false;
    let mut kbps = 0.0;

    if let Some(audio) = &plan.audio {
        let (rate, approx) = audio_kbps(media, audio)?;
        kbps += rate;
        approximate |= approx;
    }
    match &plan.video {
        Some(VideoPlan::Encode(enc)) if enc.rate == VideoRate::Palette => {
            // GIF: bytes per pixel per frame, highly content dependent.
            let (w, h) = (
                enc.width.unwrap_or(480) as f64,
                enc.height.unwrap_or(270) as f64,
            );
            let fps = enc.fps.unwrap_or(10.0);
            kbps += w * h * fps * 0.12 * 8.0 / 1000.0;
            approximate = true;
        }
        Some(video) => {
            let (rate, approx) = video_kbps(media, video, kbps)?;
            kbps += rate;
            approximate |= approx;
        }
        None => {}
    }

    let bytes = kbps * 1000.0 / 8.0 * duration * OVERHEAD;
    Some(SizeEstimate {
        bytes: bytes.round() as u64,
        approximate,
    })
}

fn audio_kbps(media: &MediaInfo, audio: &AudioPlan) -> Option<(f64, bool)> {
    let src = media.primary_audio()?;
    let channels = |enc_channels: Option<u32>| enc_channels.or(src.channels).unwrap_or(2) as f64;
    Some(match audio {
        AudioPlan::Copy { .. } => match src.bit_rate {
            Some(bps) => (bps as f64 / 1000.0, false),
            None if media.kind == MediaKind::Audio => (media.bit_rate? as f64 / 1000.0, true),
            None => (128.0, true),
        },
        AudioPlan::Encode(enc) => {
            let sample_rate = enc.sample_rate.or(src.sample_rate).unwrap_or(44100) as f64;
            match enc.rate {
                AudioRate::Bitrate { kbps } => (kbps as f64, enc.codec == AudioCodec::Opus),
                AudioRate::Vbr { quality } => {
                    let stereo = match enc.codec {
                        AudioCodec::Mp3 => lame_vbr_kbps(quality),
                        _ => vorbis_kbps(quality),
                    };
                    (stereo * channels(enc.channels).min(2.0) / 2.0, true)
                }
                AudioRate::Pcm { bits } => (
                    sample_rate * channels(enc.channels) * bits as f64 / 1000.0,
                    false,
                ),
                AudioRate::Flac { bits, .. } | AudioRate::Alac { bits } => {
                    // Typical music compresses to roughly 60% of PCM.
                    (
                        sample_rate * channels(enc.channels) * bits as f64 / 1000.0 * 0.6,
                        true,
                    )
                }
            }
        }
    })
}

fn video_kbps(media: &MediaInfo, video: &VideoPlan, audio_kbps: f64) -> Option<(f64, bool)> {
    let src = media.primary_video()?;
    Some(match video {
        VideoPlan::Copy { .. } => match src.bit_rate {
            Some(bps) => (bps as f64 / 1000.0, false),
            // Derive from the overall bit rate minus the audio.
            None => (
                (media.bit_rate? as f64 / 1000.0 - audio_kbps).max(0.0),
                true,
            ),
        },
        VideoPlan::Encode(enc) => {
            let pixels = enc.width.unwrap_or(1280) as f64 * enc.height.unwrap_or(720) as f64;
            let fps = enc.fps.or(src.fps).unwrap_or(30.0);
            let from_bpp = |bpp: f64| pixels * fps * bpp / 1000.0;
            match enc.rate {
                VideoRate::Bitrate { kbps } => (kbps as f64, false),
                VideoRate::Crf { crf, .. } => {
                    // Bits per pixel at the reference CRF, halving every 6 steps.
                    let (reference_crf, reference_bpp) = match enc.codec {
                        VideoCodec::Av1 => (34.0, 0.035),
                        _ => (31.0, 0.05),
                    };
                    let bpp = reference_bpp * 2f64.powf((reference_crf - crf as f64) / 6.0);
                    (from_bpp(bpp), true)
                }
                VideoRate::Quantizer { q } => {
                    // MPEG-2 needs about 50% more bits than MPEG-4 Part 2.
                    let reference = match enc.codec {
                        VideoCodec::Mpeg2 => 0.72,
                        _ => 0.48,
                    };
                    (from_bpp(reference / q as f64), true)
                }
                // Animated WebP: about a third of a GIF at the default quality.
                VideoRate::Quality { quality } => (
                    from_bpp(0.3 * 2f64.powf((quality as f64 - 75.0) / 15.0)),
                    true,
                ),
                // APNG: lossless RGB, only partly helped by frame differences.
                VideoRate::Lossless => (from_bpp(4.0), true),
                VideoRate::Palette => unreachable!("handled by the caller"),
            }
        }
    })
}

/// Pictures: pixels × typical bits per pixel of the encoder at that quality
/// (for a photo), corrected by how compressible the source looks. Always
/// approximate: the size depends a lot on the content.
fn estimate_picture(media: &MediaInfo, image: &ImagePlan) -> Option<SizeEstimate> {
    if !image.icons.is_empty() {
        // Small PNG pictures (about 2 bytes per pixel) plus the icon directory.
        let pixels: f64 = image.icons.iter().map(|i| (i.size * i.size) as f64).sum();
        return Some(SizeEstimate {
            bytes: (pixels * 2.0) as u64 + 22 + 16 * image.icons.len() as u64,
            approximate: true,
        });
    }
    let pixels = image.width? as f64 * image.height? as f64;
    if image.codec == ImageCodec::Bmp {
        // Uncompressed: 3 bytes per pixel, rows padded to 4 bytes, 54-byte header.
        let row = (image.width? as u64 * 3).div_ceil(4) * 4;
        return Some(SizeEstimate {
            bytes: row * image.height? as u64 + 54,
            approximate: false,
        });
    }
    let bits_per_pixel = match (image.codec, image.quality) {
        (ImageCodec::Tiff, _) => {
            if image.alpha {
                18.0
            } else {
                14.0
            }
        }
        (_, ImageQuality::Palette) => 6.0,
        (ImageCodec::Jxl, ImageQuality::Lossless { .. }) => {
            if image.alpha {
                13.0
            } else {
                10.0
            }
        }
        (ImageCodec::Jxl, ImageQuality::Lossy { quality }) => jpeg_bits_per_pixel(quality) * 0.6,
        (ImageCodec::Png, _) => {
            if image.alpha {
                16.0
            } else {
                12.0
            }
        }
        (ImageCodec::Webp, ImageQuality::Lossless { .. }) => {
            if image.alpha {
                12.0
            } else {
                9.0
            }
        }
        (ImageCodec::Webp, ImageQuality::Lossy { quality }) => jpeg_bits_per_pixel(quality) * 0.7,
        (ImageCodec::Avif, ImageQuality::Lossy { quality }) => {
            0.5 * 2f64.powf((quality as f64 - 60.0) / 10.0)
        }
        (_, ImageQuality::Lossy { quality }) => jpeg_bits_per_pixel(quality),
        (_, ImageQuality::Lossless { .. }) => 12.0,
    };
    let bytes = pixels * bits_per_pixel * content_factor(media) / 8.0 + 500.0;
    Some(SizeEstimate {
        bytes: bytes.round() as u64,
        approximate: true,
    })
}

/// Average JPEG bits per pixel of a photo at a quality from 1 to 100.
fn jpeg_bits_per_pixel(quality: u8) -> f64 {
    const POINTS: [(f64, f64); 6] = [
        (1.0, 0.15),
        (50.0, 1.0),
        (70.0, 1.5),
        (85.0, 2.4),
        (95.0, 4.5),
        (100.0, 7.0),
    ];
    let q = (quality as f64).clamp(1.0, 100.0);
    POINTS
        .windows(2)
        .find(|w| q <= w[1].0)
        .map_or(POINTS[5].1, |w| {
            let ((q0, b0), (q1, b1)) = (w[0], w[1]);
            b0 + (b1 - b0) * (q - q0) / (q1 - q0)
        })
}

/// How the source compares with a typical photo in the same format: a simple
/// drawing compresses much better than a photo in every format (damped by a
/// square root, since formats do not react alike).
fn content_factor(media: &MediaInfo) -> f64 {
    let Some(src) = media.primary_video() else {
        return 1.0;
    };
    let typical = match src.codec.as_str() {
        "mjpeg" => 2.4,
        "webp" if is_lossless_image("webp", src.pix_fmt.as_deref()) => 9.0,
        "webp" => 1.6,
        "png" => 12.0,
        "hevc" => 0.9,
        "av1" => 0.6,
        _ => return 1.0,
    };
    match (media.size_bytes, src.width, src.height) {
        (Some(bytes), Some(w), Some(h)) if w > 0 && h > 0 => {
            let bits_per_pixel = bytes as f64 * 8.0 / (w as f64 * h as f64);
            (bits_per_pixel / typical).sqrt().clamp(0.25, 2.0)
        }
        _ => 1.0,
    }
}

/// Average bit rate of LAME VBR presets (stereo, 44.1 kHz).
fn lame_vbr_kbps(quality: i8) -> f64 {
    const TABLE: [f64; 10] = [
        245.0, 225.0, 190.0, 175.0, 165.0, 130.0, 115.0, 100.0, 85.0, 65.0,
    ];
    TABLE[quality.clamp(0, 9) as usize]
}

/// Nominal bit rate of Vorbis quality levels −1 … 10 (stereo, 44.1 kHz).
fn vorbis_kbps(quality: i8) -> f64 {
    const TABLE: [f64; 12] = [
        45.0, 64.0, 80.0, 96.0, 112.0, 128.0, 160.0, 192.0, 224.0, 256.0, 320.0, 500.0,
    ];
    TABLE[(quality.clamp(-1, 10) + 1) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::TargetFormat;
    use crate::plan::plan_conversion;
    use crate::probe::parse_ffprobe_json;
    use crate::request::{ConversionRequest, QualityPreset};
    use std::path::Path;

    fn wav_cd_60s() -> MediaInfo {
        parse_ffprobe_json(
            br#"{ "streams": [{ "index": 0, "codec_name": "pcm_s16le", "codec_type": "audio",
                "sample_rate": "44100", "channels": 2, "bits_per_sample": 16 }],
                "format": { "format_name": "wav", "duration": "60" } }"#,
        )
        .unwrap()
    }

    fn estimate(
        media: &MediaInfo,
        target: TargetFormat,
        preset: QualityPreset,
        input: &str,
    ) -> SizeEstimate {
        plan_conversion(
            media,
            &ConversionRequest::new(target, preset),
            Path::new(input),
        )
        .unwrap()
        .estimate
        .unwrap()
    }

    #[test]
    fn pcm_estimate_is_exact() {
        // 44100 Hz × 2 ch × 16 bit = 1411.2 kbit/s → 60 s = 10 584 000 bytes (+1% overhead).
        let e = estimate(
            &wav_cd_60s(),
            TargetFormat::Aiff,
            QualityPreset::Balanced,
            "a.wav",
        );
        assert_eq!(
            e,
            SizeEstimate {
                bytes: 10_689_840,
                approximate: false
            }
        );
    }

    #[test]
    fn mp3_vbr_estimate_is_approximate() {
        let e = estimate(
            &wav_cd_60s(),
            TargetFormat::Mp3,
            QualityPreset::Balanced,
            "a.wav",
        );
        assert!(e.approximate);
        // ~190 kbit/s × 60 s ≈ 1.44 MB.
        assert!((1_400_000..1_500_000).contains(&e.bytes), "{}", e.bytes);
    }

    #[test]
    fn smaller_presets_give_smaller_estimates() {
        let media = wav_cd_60s();
        for target in [
            TargetFormat::Mp3,
            TargetFormat::M4a,
            TargetFormat::Ogg,
            TargetFormat::Opus,
        ] {
            let sizes: Vec<u64> = [
                QualityPreset::Max,
                QualityPreset::Balanced,
                QualityPreset::Small,
            ]
            .into_iter()
            .map(|p| estimate(&media, target, p, "a.wav").bytes)
            .collect();
            assert!(
                sizes[0] > sizes[1] && sizes[1] > sizes[2],
                "{target:?}: {sizes:?}"
            );
        }
    }

    #[test]
    fn copy_uses_the_source_bit_rates() {
        let media = parse_ffprobe_json(
            br#"{ "streams": [
                { "index": 0, "codec_name": "h264", "codec_type": "video", "width": 1280, "height": 720,
                  "bit_rate": "2000000" },
                { "index": 1, "codec_name": "aac", "codec_type": "audio", "bit_rate": "128000" }],
                "format": { "format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "10" } }"#,
        )
        .unwrap();
        let e = estimate(
            &media,
            TargetFormat::Mkv,
            QualityPreset::Balanced,
            "clip.mp4",
        );
        // (2000 + 128) kbit/s × 10 s = 2.66 MB, +1%.
        assert_eq!(
            e,
            SizeEstimate {
                bytes: 2_686_600,
                approximate: false
            }
        );
    }

    fn picture(codec: &str, container: &str, bytes: u64) -> MediaInfo {
        parse_ffprobe_json(
            format!(
                r#"{{ "streams": [{{ "index": 0, "codec_name": "{codec}", "codec_type": "video",
                "width": 4000, "height": 3000, "pix_fmt": "yuvj420p" }}],
                "format": {{ "format_name": "{container}", "size": "{bytes}" }} }}"#
            )
            .as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn picture_estimates_follow_quality_and_content() {
        // A typical 12 MP photo (2.4 bits per pixel).
        let photo = picture("mjpeg", "image2", 3_600_000);
        let size = |target, preset| estimate(&photo, target, preset, "a.jpg");
        let jpg = size(TargetFormat::Jpg, QualityPreset::Balanced);
        assert!(jpg.approximate);
        assert!((3_000_000..4_200_000).contains(&jpg.bytes), "{}", jpg.bytes);
        for target in [TargetFormat::Jpg, TargetFormat::Webp, TargetFormat::Avif] {
            let sizes = [
                QualityPreset::Max,
                QualityPreset::Balanced,
                QualityPreset::Small,
            ]
            .map(|p| size(target, p).bytes);
            assert!(
                sizes[0] > sizes[1] && sizes[1] > sizes[2],
                "{target:?}: {sizes:?}"
            );
        }
        assert!(size(TargetFormat::Png, QualityPreset::Balanced).bytes > jpg.bytes * 3);
        assert!(size(TargetFormat::Avif, QualityPreset::Balanced).bytes < jpg.bytes / 2);

        // A simple drawing compresses much better than a photo.
        let drawing = picture("mjpeg", "image2", 600_000);
        let small = estimate(
            &drawing,
            TargetFormat::Jpg,
            QualityPreset::Balanced,
            "a.jpg",
        );
        assert!(
            small.bytes < jpg.bytes / 2,
            "{} vs {}",
            small.bytes,
            jpg.bytes
        );
    }

    #[test]
    fn unknown_duration_gives_no_estimate() {
        let mut media = wav_cd_60s();
        media.duration_secs = None;
        let plan = plan_conversion(
            &media,
            &ConversionRequest::new(TargetFormat::Mp3, QualityPreset::Balanced),
            Path::new("a.wav"),
        )
        .unwrap();
        assert_eq!(plan.estimate, None);
    }
}
