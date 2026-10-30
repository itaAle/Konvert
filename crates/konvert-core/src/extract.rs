//! Tracks of an audio or video file saved as files of their own: audio in
//! an audio format (copied when the format holds its codec), the video in
//! the file's own container without audio, subtitles as SRT, WebVTT or ASS
//! (text subtitles only). A trim chosen for the file applies to each.

use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::CoreError;
use crate::formats::{self, FormatKind, TargetFormat};
use crate::plan::{ConversionPlan, plan_offered};
use crate::probe::{MediaInfo, Track, TrackKind};
use crate::request::{ConversionRequest, ExtractOptions, MediaEdit, SubtitleFormat, TrackChoice};

/// One file to be written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlannedExtract {
    /// Stream index in the input.
    pub index: u32,
    pub kind: TrackKind,
    /// Extension of the file.
    pub extension: String,
    /// Word after the file's name (`ita`, `audio2`, `video`).
    pub label: String,
    /// Audio or video: how the track is written.
    #[ts(skip)]
    #[serde(skip)]
    pub plan: Option<ConversionPlan>,
    /// Subtitles: the codec they are written in.
    pub subtitle_codec: Option<String>,
}

impl SubtitleFormat {
    pub fn extension(self) -> &'static str {
        match self {
            SubtitleFormat::Srt => "srt",
            SubtitleFormat::Vtt => "vtt",
            SubtitleFormat::Ass => "ass",
        }
    }

    /// FFmpeg encoder and muxer.
    pub(crate) fn codec(self) -> (&'static str, &'static str) {
        match self {
            SubtitleFormat::Srt => ("subrip", "srt"),
            SubtitleFormat::Vtt => ("webvtt", "webvtt"),
            SubtitleFormat::Ass => ("ass", "ass"),
        }
    }
}

/// The audio format that keeps an audio codec as it is (copied), or FLAC
/// (lossless) when none does.
pub fn audio_format_for(codec: &str) -> TargetFormat {
    use TargetFormat as F;
    match codec {
        "aac" => F::M4a,
        "mp3" => F::Mp3,
        "opus" => F::Opus,
        "vorbis" => F::Ogg,
        "flac" => F::Flac,
        "alac" => F::Alac,
        "ac3" => F::Ac3,
        "eac3" => F::Eac3,
        "mp2" => F::Mp2,
        "wmav2" => F::Wma,
        c if c.starts_with("pcm_") => F::Wav,
        _ => F::Flac,
    }
}

/// The subtitle format that keeps a subtitle codec as it is: ASS for ASS/SSA,
/// WebVTT for WebVTT, otherwise SRT.
pub fn subtitle_format_for(codec: &str) -> SubtitleFormat {
    match codec {
        "ass" | "ssa" => SubtitleFormat::Ass,
        "webvtt" => SubtitleFormat::Vtt,
        _ => SubtitleFormat::Srt,
    }
}

/// The files the chosen tracks become, in order.
pub(crate) fn plan(
    media: &MediaInfo,
    request: &ConversionRequest,
    options: &ExtractOptions,
    input: &Path,
) -> Result<Vec<PlannedExtract>, CoreError> {
    if options.tracks.is_empty() {
        return Err(unsupported("no track to extract".into()));
    }
    let trim = request.media.as_ref().and_then(|m| m.trim);
    let mut labels: Vec<String> = Vec::new();
    let mut out = Vec::with_capacity(options.tracks.len());
    for chosen in &options.tracks {
        let track = media
            .tracks
            .iter()
            .find(|t| t.index == chosen.index)
            .ok_or_else(|| unsupported(format!("there is no track {}", chosen.index)))?;
        let label = unique_label(&mut labels, base_label(media, track));
        let single = |target: TargetFormat| ConversionRequest {
            target,
            media: Some(MediaEdit {
                tracks: Some(vec![TrackChoice {
                    index: track.index,
                    language: track.language.clone(),
                    title: track.title.clone(),
                    default: true,
                }]),
                trim,
                audio_file: None,
                // Copied when it can be: never re-encoded only because the
                // container is the same.
                copy: true,
                label: None,
            }),
            compress: None,
            extract: None,
            edit: None,
            pdf: None,
            ..request.clone()
        };
        let planned = match track.kind {
            TrackKind::Audio => {
                let target = chosen
                    .audio
                    .unwrap_or_else(|| audio_format_for(&track.codec));
                if formats::spec(target).kind != FormatKind::Audio {
                    return Err(unsupported(format!("{target:?} is not an audio format")));
                }
                PlannedExtract {
                    index: track.index,
                    kind: track.kind,
                    extension: formats::spec(target).extension.into(),
                    label,
                    plan: Some(plan_offered(media, &single(target), input)?),
                    subtitle_codec: None,
                }
            }
            TrackKind::Video => {
                let target = formats::source_format(media, input)
                    .filter(|f| formats::spec(*f).kind == FormatKind::Video)
                    .unwrap_or(TargetFormat::Mkv);
                PlannedExtract {
                    index: track.index,
                    kind: track.kind,
                    extension: formats::spec(target).extension.into(),
                    label,
                    plan: Some(plan_offered(media, &single(target), input)?),
                    subtitle_codec: None,
                }
            }
            TrackKind::Subtitle => {
                if track.bitmap {
                    return Err(unsupported(
                        "subtitles drawn as pictures cannot become text".into(),
                    ));
                }
                let format = chosen
                    .subtitles
                    .unwrap_or_else(|| subtitle_format_for(&track.codec));
                PlannedExtract {
                    index: track.index,
                    kind: track.kind,
                    extension: format.extension().into(),
                    label,
                    plan: None,
                    subtitle_codec: Some(format.codec().0.into()),
                }
            }
        };
        out.push(planned);
    }
    Ok(out)
}

/// `ita`, else `audio2`, `video`, `subtitles1`.
fn base_label(media: &MediaInfo, track: &Track) -> String {
    let clean: String = track
        .language
        .as_deref()
        .unwrap_or_default()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(8)
        .collect();
    if !clean.is_empty() {
        return clean.to_ascii_lowercase();
    }
    let position = media
        .tracks
        .iter()
        .filter(|t| t.kind == track.kind)
        .position(|t| t.index == track.index)
        .unwrap_or(0)
        + 1;
    match track.kind {
        TrackKind::Video => "video".into(),
        TrackKind::Audio => format!("audio{position}"),
        TrackKind::Subtitle => format!("subtitles{position}"),
    }
}

/// `ita`, then `ita-2`, `ita-3`…
fn unique_label(used: &mut Vec<String>, base: String) -> String {
    let mut label = base.clone();
    let mut n = 2;
    while used.contains(&label) {
        label = format!("{base}-{n}");
        n += 1;
    }
    used.push(label.clone());
    label
}

fn unsupported(details: String) -> CoreError {
    CoreError::UnsupportedConversion { details }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::parse_ffprobe_json;
    use crate::request::{ExtractTrack, QualityPreset};

    fn film() -> MediaInfo {
        parse_ffprobe_json(
            br#"{ "streams": [
            { "index": 0, "codec_name": "h264", "codec_type": "video", "width": 1920, "height": 1080 },
            { "index": 1, "codec_name": "aac", "codec_type": "audio", "channels": 2 },
            { "index": 2, "codec_name": "dts", "codec_type": "audio", "channels": 6, "tags": { "language": "eng" } },
            { "index": 3, "codec_name": "ass", "codec_type": "subtitle" },
            { "index": 4, "codec_name": "hdmv_pgs_subtitle", "codec_type": "subtitle" }],
            "format": { "format_name": "matroska,webm", "duration": "60" } }"#,
        )
        .unwrap()
    }

    fn extract(tracks: &[u32]) -> Result<Vec<PlannedExtract>, CoreError> {
        let options = ExtractOptions {
            tracks: tracks
                .iter()
                .map(|&index| ExtractTrack {
                    index,
                    audio: None,
                    subtitles: None,
                })
                .collect(),
        };
        let request = ConversionRequest::new(TargetFormat::Mkv, QualityPreset::Balanced);
        plan(&film(), &request, &options, Path::new("film.mkv"))
    }

    #[test]
    fn tracks_keep_their_codec_where_a_format_holds_it() {
        let files = extract(&[0, 1, 2, 3]).unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|f| format!("{}.{}", f.label, f.extension))
            .collect();
        assert_eq!(
            names,
            ["video.mkv", "audio1.m4a", "eng.flac", "subtitles1.ass"]
        );
        // AAC into M4A is copied, DTS has no audio format: lossless FLAC.
        assert!(files[1].plan.as_ref().unwrap().is_remux());
        assert!(!files[2].plan.as_ref().unwrap().is_remux());
        assert!(files[0].plan.as_ref().unwrap().is_remux());
        assert_eq!(files[3].subtitle_codec.as_deref(), Some("ass"));
        assert_eq!(audio_format_for("pcm_s24le"), TargetFormat::Wav);
        assert_eq!(subtitle_format_for("mov_text"), SubtitleFormat::Srt);
    }

    #[test]
    fn picture_subtitles_and_missing_tracks_are_refused() {
        assert!(extract(&[4]).is_err());
        assert!(extract(&[9]).is_err());
        assert!(extract(&[]).is_err());
    }

    #[test]
    fn labels_are_unique() {
        let mut used = Vec::new();
        assert_eq!(unique_label(&mut used, "ita".into()), "ita");
        assert_eq!(unique_label(&mut used, "ita".into()), "ita-2");
        assert_eq!(unique_label(&mut used, "ita".into()), "ita-3");
    }
}
