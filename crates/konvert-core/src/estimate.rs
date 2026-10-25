//! Estimated size of the converted file.
//!
//! Exact for constant bit rates and PCM, approximate for VBR, constant-quality
//! and lossless compression (the UI shows those as "circa").

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::formats::{AudioCodec, VideoCodec};
use crate::plan::{AudioPlan, AudioRate, ConversionPlan, VideoPlan, VideoRate};
use crate::probe::{MediaInfo, MediaKind};

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

/// Estimates the output size; `None` when the duration is unknown.
pub fn estimate_size(media: &MediaInfo, plan: &ConversionPlan) -> Option<SizeEstimate> {
    let duration = media.duration_secs?;
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
                AudioRate::Flac { bits, .. } => {
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
                VideoRate::Quantizer { q } => (from_bpp(0.48 / q as f64), true),
                VideoRate::Palette => unreachable!("handled by the caller"),
            }
        }
    })
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
