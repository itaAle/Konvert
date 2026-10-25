//! Media analysis with FFprobe.
//!
//! [`parse_ffprobe_json`] is a pure function (unit-tested with captured output);
//! [`probe`] runs FFprobe on a local file and feeds its JSON to the parser.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::binaries::Binaries;
use crate::error::CoreError;
use crate::process;

/// What kind of content a file holds, which drives the proposed targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaKind {
    /// Audio only (cover art pictures do not count as video).
    Audio,
    /// Video without any audio track.
    Video,
    /// Video with at least one audio track.
    VideoWithAudio,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStream {
    pub index: u32,
    pub codec: String,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub bit_rate: Option<u64>,
    /// Bits per sample for PCM/lossless sources (e.g. 16, 24).
    pub bit_depth: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStream {
    pub index: u32,
    pub codec: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub bit_rate: Option<u64>,
    pub pix_fmt: Option<String>,
}

/// Result of analysing an input file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    /// FFmpeg demuxer name(s), e.g. `wav` or `mov,mp4,m4a,3gp,3g2,mj2`.
    pub container: String,
    pub kind: MediaKind,
    pub duration_secs: Option<f64>,
    pub size_bytes: Option<u64>,
    /// Overall bit rate in bits per second.
    pub bit_rate: Option<u64>,
    pub audio: Vec<AudioStream>,
    /// Real video streams (attached cover pictures excluded).
    pub video: Vec<VideoStream>,
    /// The file embeds a cover picture (e.g. album art in MP3/M4A/FLAC).
    pub has_cover_art: bool,
    pub subtitle_streams: u32,
}

impl MediaInfo {
    pub fn primary_audio(&self) -> Option<&AudioStream> {
        self.audio.first()
    }

    pub fn primary_video(&self) -> Option<&VideoStream> {
        self.video.first()
    }
}

/// Runs FFprobe on a local file. Network protocols are not allowed.
pub async fn probe(bins: &Binaries, input: &Path) -> Result<MediaInfo, CoreError> {
    if !input.is_file() {
        return Err(CoreError::InputNotFound);
    }
    let mut cmd = process::engine_command(&bins.ffprobe);
    cmd.args(["-hide_banner", "-v", "error"])
        .args(["-protocol_whitelist", "file"])
        .args(["-print_format", "json", "-show_format", "-show_streams"])
        .arg(crate::command::file_url(input));
    let output = process::spawn(&mut cmd)?.wait_with_output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(match CoreError::from_engine_output(&stderr) {
            CoreError::EngineFailed { details } => CoreError::CorruptInput { details },
            other => other,
        });
    }
    parse_ffprobe_json(&output.stdout)
}

/// Converts FFprobe's `-print_format json -show_format -show_streams` output.
pub fn parse_ffprobe_json(json: &[u8]) -> Result<MediaInfo, CoreError> {
    let raw: RawProbe = serde_json::from_slice(json).map_err(|e| CoreError::CorruptInput {
        details: format!("unexpected ffprobe output: {e}"),
    })?;
    let format = raw.format.unwrap_or_default();
    let container = format.format_name.unwrap_or_default();

    let mut audio = Vec::new();
    let mut video = Vec::new();
    let mut has_cover_art = false;
    let mut subtitle_streams = 0;

    for s in raw.streams {
        let codec = s.codec_name.clone().unwrap_or_default();
        match s.codec_type.as_deref() {
            Some("audio") => audio.push(AudioStream {
                index: s.index,
                codec,
                sample_rate: parse_num(&s.sample_rate),
                channels: s.channels,
                bit_rate: parse_num(&s.bit_rate),
                bit_depth: s
                    .bits_per_raw_sample
                    .as_deref()
                    .and_then(|v| v.parse().ok())
                    .or(s.bits_per_sample)
                    .filter(|b| *b > 0),
            }),
            Some("video") if s.is_attached_picture() => has_cover_art = true,
            Some("video") => video.push(VideoStream {
                index: s.index,
                codec,
                width: s.width,
                height: s.height,
                fps: parse_rate(s.avg_frame_rate.as_deref())
                    .or_else(|| parse_rate(s.r_frame_rate.as_deref())),
                bit_rate: parse_num(&s.bit_rate),
                pix_fmt: s.pix_fmt,
            }),
            Some("subtitle") => subtitle_streams += 1,
            _ => {}
        }
    }

    if is_still_image(&container, &video, audio.is_empty()) {
        return Err(CoreError::NotMedia);
    }

    let kind = match (video.is_empty(), audio.is_empty()) {
        (false, false) => MediaKind::VideoWithAudio,
        (false, true) => MediaKind::Video,
        (true, false) => MediaKind::Audio,
        (true, true) => return Err(CoreError::NotMedia),
    };

    Ok(MediaInfo {
        container,
        kind,
        duration_secs: parse_num::<f64>(&format.duration).filter(|d| d.is_finite() && *d > 0.0),
        size_bytes: parse_num(&format.size),
        bit_rate: parse_num(&format.bit_rate),
        audio,
        video,
        has_cover_art,
        subtitle_streams,
    })
}

/// Single pictures (PNG, JPEG, ...) are not media Konvert converts.
fn is_still_image(container: &str, video: &[VideoStream], no_audio: bool) -> bool {
    let image_demuxer = container == "image2" || container.ends_with("_pipe");
    let animated = video
        .iter()
        .any(|v| matches!(v.codec.as_str(), "gif" | "apng" | "webp"));
    no_audio && image_demuxer && !animated
}

fn parse_num<T: std::str::FromStr>(v: &Option<String>) -> Option<T> {
    v.as_deref().and_then(|s| s.parse().ok())
}

/// Parses FFprobe rationals like `30000/1001`; `0/0` means unknown.
fn parse_rate(v: Option<&str>) -> Option<f64> {
    let (num, den) = v?.split_once('/')?;
    let (num, den): (f64, f64) = (num.parse().ok()?, den.parse().ok()?);
    (num > 0.0 && den > 0.0).then(|| num / den)
}

// --- Raw FFprobe JSON -------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawProbe {
    #[serde(default)]
    streams: Vec<RawStream>,
    format: Option<RawFormat>,
}

#[derive(Debug, Default, Deserialize)]
struct RawFormat {
    format_name: Option<String>,
    duration: Option<String>,
    size: Option<String>,
    bit_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawStream {
    index: u32,
    codec_name: Option<String>,
    codec_type: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    bit_rate: Option<String>,
    bits_per_sample: Option<u32>,
    bits_per_raw_sample: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    pix_fmt: Option<String>,
    #[serde(default)]
    disposition: HashMap<String, i64>,
}

impl RawStream {
    fn is_attached_picture(&self) -> bool {
        self.disposition.get("attached_pic").copied() == Some(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WAV: &str = r#"{
      "streams": [{ "index": 0, "codec_name": "pcm_s16le", "codec_type": "audio",
        "sample_rate": "44100", "channels": 2, "bits_per_sample": 16, "bit_rate": "1411200",
        "disposition": { "default": 0, "attached_pic": 0 } }],
      "format": { "format_name": "wav", "duration": "3.000000", "size": "529244", "bit_rate": "1411312" }
    }"#;

    const MP3_WITH_COVER: &str = r#"{
      "streams": [
        { "index": 0, "codec_name": "mp3", "codec_type": "audio", "sample_rate": "48000",
          "channels": 2, "bit_rate": "320000", "disposition": { "attached_pic": 0 } },
        { "index": 1, "codec_name": "mjpeg", "codec_type": "video", "width": 600, "height": 600,
          "avg_frame_rate": "0/0", "r_frame_rate": "90000/1", "disposition": { "attached_pic": 1 } }
      ],
      "format": { "format_name": "mp3", "duration": "215.4", "size": "8616000", "bit_rate": "320000" }
    }"#;

    const MP4: &str = r#"{
      "streams": [
        { "index": 0, "codec_name": "h264", "codec_type": "video", "width": 1920, "height": 1080,
          "avg_frame_rate": "30000/1001", "pix_fmt": "yuv420p", "bit_rate": "4500000",
          "disposition": { "attached_pic": 0 } },
        { "index": 1, "codec_name": "aac", "codec_type": "audio", "sample_rate": "48000",
          "channels": 6, "bit_rate": "384000", "disposition": { "attached_pic": 0 } },
        { "index": 2, "codec_name": "mov_text", "codec_type": "subtitle" }
      ],
      "format": { "format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "60.06", "size": "36000000" }
    }"#;

    #[test]
    fn wav_is_audio() {
        let info = parse_ffprobe_json(WAV.as_bytes()).unwrap();
        assert_eq!(info.kind, MediaKind::Audio);
        assert_eq!(info.container, "wav");
        assert_eq!(info.duration_secs, Some(3.0));
        let a = info.primary_audio().unwrap();
        assert_eq!(
            (a.codec.as_str(), a.sample_rate, a.channels),
            ("pcm_s16le", Some(44100), Some(2))
        );
        assert_eq!(a.bit_depth, Some(16));
    }

    #[test]
    fn cover_art_does_not_make_a_video() {
        let info = parse_ffprobe_json(MP3_WITH_COVER.as_bytes()).unwrap();
        assert_eq!(info.kind, MediaKind::Audio);
        assert!(info.has_cover_art);
        assert!(info.video.is_empty());
    }

    #[test]
    fn mp4_is_video_with_audio() {
        let info = parse_ffprobe_json(MP4.as_bytes()).unwrap();
        assert_eq!(info.kind, MediaKind::VideoWithAudio);
        let v = info.primary_video().unwrap();
        assert_eq!((v.width, v.height), (Some(1920), Some(1080)));
        assert!((v.fps.unwrap() - 29.97).abs() < 0.01);
        assert_eq!(info.subtitle_streams, 1);
        assert_eq!(info.bit_rate, None);
    }

    #[test]
    fn still_images_and_empty_files_are_not_media() {
        let png = r#"{ "streams": [{ "index": 0, "codec_name": "png", "codec_type": "video",
            "width": 10, "height": 10 }], "format": { "format_name": "png_pipe" } }"#;
        assert!(matches!(
            parse_ffprobe_json(png.as_bytes()),
            Err(CoreError::NotMedia)
        ));
        let empty = r#"{ "streams": [], "format": { "format_name": "mp3" } }"#;
        assert!(matches!(
            parse_ffprobe_json(empty.as_bytes()),
            Err(CoreError::NotMedia)
        ));
    }

    #[test]
    fn garbage_is_reported_as_corrupt() {
        assert!(matches!(
            parse_ffprobe_json(b"not json"),
            Err(CoreError::CorruptInput { .. })
        ));
    }
}
