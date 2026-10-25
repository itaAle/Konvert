//! FFmpeg command builder.
//!
//! [`build_command`] is a pure function: analysed input + user choices + paths in,
//! argument vector out. It never touches the file system or spawns processes,
//! which keeps it fully unit-testable. The UI never sends raw FFmpeg arguments:
//! it sends a [`ConversionRequest`] and the arguments are derived here.

use std::ffi::OsString;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::probe::MediaInfo;

/// Output formats Konvert can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetFormat {
    Mp3,
}

impl TargetFormat {
    /// File extension of the produced file.
    pub fn extension(self) -> &'static str {
        match self {
            TargetFormat::Mp3 => "mp3",
        }
    }

    /// FFmpeg muxer name, passed explicitly with `-f`.
    pub fn muxer(self) -> &'static str {
        match self {
            TargetFormat::Mp3 => "mp3",
        }
    }

    /// Parses an extension-like identifier (`"mp3"`, `"MP3"`).
    pub fn from_id(id: &str) -> Option<Self> {
        match id.to_ascii_lowercase().as_str() {
            "mp3" => Some(TargetFormat::Mp3),
            _ => None,
        }
    }
}

/// Plain-language quality choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QualityPreset {
    /// "Massima qualità"
    Max,
    /// "Bilanciata"
    #[default]
    Balanced,
    /// "File piccolo"
    Small,
}

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionRequest {
    pub target: TargetFormat,
    #[serde(default)]
    pub preset: QualityPreset,
}

/// A fully built FFmpeg invocation (program arguments only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegCommand {
    pub args: Vec<OsString>,
}

impl FfmpegCommand {
    /// Arguments as UTF-8 strings (lossy), handy for tests and debug output.
    pub fn args_lossy(&self) -> Vec<String> {
        self.args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }
}

/// `file:` URL for a local path.
///
/// The explicit protocol makes FFmpeg treat the argument as a local file even if
/// the name starts with `-` or looks like another protocol (`http:...`), and it
/// is the only protocol allowed by `-protocol_whitelist`.
pub fn file_url(path: &Path) -> OsString {
    let mut url = OsString::from("file:");
    url.push(path.as_os_str());
    url
}

/// Builds the FFmpeg arguments to convert `input` into `output`.
pub fn build_command(
    media: &MediaInfo,
    request: &ConversionRequest,
    input: &Path,
    output: &Path,
) -> Result<FfmpegCommand, CoreError> {
    let mut args: Vec<OsString> = Vec::new();

    // Global options: quiet banner, no interactive stdin, machine-readable progress.
    push(
        &mut args,
        &[
            "-hide_banner",
            "-nostdin",
            "-nostats",
            "-loglevel",
            "warning",
        ],
    );
    push(&mut args, &["-progress", "pipe:1"]);

    // Input: local files only.
    push(&mut args, &["-protocol_whitelist", "file", "-i"]);
    args.push(file_url(input));

    if media.audio.is_empty() {
        return Err(CoreError::UnsupportedConversion {
            details: "the input has no audio track".into(),
        });
    }

    match request.target {
        TargetFormat::Mp3 => {
            push(&mut args, &["-map", "0:a:0", "-map_metadata", "0"]);
            push(
                &mut args,
                &[
                    "-c:a",
                    "libmp3lame",
                    "-q:a",
                    mp3_vbr_quality(request.preset),
                ],
            );
            push(&mut args, &["-id3v2_version", "3"]);
        }
    }

    push(&mut args, &["-f", request.target.muxer(), "-y"]);
    args.push(file_url(output));
    Ok(FfmpegCommand { args })
}

fn push(args: &mut Vec<OsString>, items: &[&str]) {
    args.extend(items.iter().map(OsString::from));
}

/// LAME VBR quality (`-q:a`): 0 ≈ 245 kbit/s, 2 ≈ 190 kbit/s, 5 ≈ 130 kbit/s.
fn mp3_vbr_quality(preset: QualityPreset) -> &'static str {
    match preset {
        QualityPreset::Max => "0",
        QualityPreset::Balanced => "2",
        QualityPreset::Small => "5",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::parse_ffprobe_json;

    fn wav() -> MediaInfo {
        parse_ffprobe_json(
            br#"{ "streams": [{ "index": 0, "codec_name": "pcm_s16le", "codec_type": "audio",
                "sample_rate": "44100", "channels": 2 }],
                "format": { "format_name": "wav", "duration": "3.0" } }"#,
        )
        .unwrap()
    }

    #[test]
    #[rustfmt::skip] // keep related arguments on the same line
    fn wav_to_mp3_balanced() {
        let req = ConversionRequest {
            target: TargetFormat::Mp3,
            preset: QualityPreset::Balanced,
        };
        let cmd = build_command(&wav(), &req, Path::new("in put.wav"), Path::new("out.mp3")).unwrap();
        assert_eq!(
            cmd.args_lossy(),
            [
                "-hide_banner", "-nostdin", "-nostats", "-loglevel", "warning",
                "-progress", "pipe:1",
                "-protocol_whitelist", "file", "-i", "file:in put.wav",
                "-map", "0:a:0", "-map_metadata", "0",
                "-c:a", "libmp3lame", "-q:a", "2",
                "-id3v2_version", "3",
                "-f", "mp3", "-y", "file:out.mp3",
            ]
        );
    }

    #[test]
    fn file_names_starting_with_a_dash_stay_file_urls() {
        let req = ConversionRequest {
            target: TargetFormat::Mp3,
            preset: QualityPreset::Small,
        };
        let cmd = build_command(&wav(), &req, Path::new("-y"), Path::new("http:x.mp3")).unwrap();
        let args = cmd.args_lossy();
        assert!(args.contains(&"file:-y".to_string()));
        assert_eq!(args.last().unwrap(), "file:http:x.mp3");
    }
}
