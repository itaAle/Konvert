//! Several audio or video files joined into one, in the order of the list.
//! When every file has the same codecs and parameters, the streams are
//! copied through FFmpeg's concat demuxer; otherwise everything is encoded
//! again at the size and frame rate of the first file (other pictures fitted
//! with black bars), the audio at 48 kHz stereo, with silence where a file
//! has none.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::CoreError;
use crate::estimate::estimate_size;
use crate::formats::{self, FormatKind};
use crate::plan::{
    AudioPlan, ConversionPlan, ConversionWarning, VideoPlan, plan_audio, plan_video,
};
use crate::probe::{AudioStream, MediaInfo, MediaKind, VideoStream};
use crate::request::{ConversionRequest, QualityPreset};

/// Frame rate of the result when the first file does not say.
const DEFAULT_FPS: f64 = 30.0;

/// How the files are joined.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JoinPlan {
    /// The streams are copied (same codecs and parameters in every file).
    pub copy: bool,
    /// Every file, in order.
    pub files: Vec<JoinedFile>,
    /// Size and frame rate of an encoded video (the first file's).
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
}

/// One of the files joined: the streams used and how long it lasts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JoinedFile {
    pub video_stream: Option<u32>,
    pub audio_stream: Option<u32>,
    pub duration_secs: f64,
}

fn unsupported(details: &str) -> CoreError {
    CoreError::UnsupportedConversion {
        details: details.into(),
    }
}

/// The video streams match: they can follow one another without encoding.
fn same_video(a: &VideoStream, b: &VideoStream) -> bool {
    let fps = match (a.fps, b.fps) {
        (Some(x), Some(y)) => (x - y).abs() < 0.01,
        (x, y) => x == y,
    };
    a.codec == b.codec
        && a.width == b.width
        && a.height == b.height
        && a.pix_fmt == b.pix_fmt
        && fps
}

fn same_audio(a: &AudioStream, b: &AudioStream) -> bool {
    a.codec == b.codec && a.sample_rate == b.sample_rate && a.channels == b.channels
}

/// Plans `files` (what the analysis found, in order) joined into one file of
/// `request.target`.
pub fn plan(files: &[MediaInfo], request: &ConversionRequest) -> Result<ConversionPlan, CoreError> {
    if files.len() < 2 {
        return Err(unsupported("at least two files are joined"));
    }
    let spec = formats::spec(request.target);
    let video_target = match spec.kind {
        FormatKind::Video => true,
        FormatKind::Audio => false,
        _ => return Err(unsupported("files are joined into audio or video")),
    };
    if spec.default_video_codec().is_some_and(|c| c.is_animation()) {
        return Err(unsupported("animations cannot be joined"));
    }
    let mut joined = Vec::with_capacity(files.len());
    for media in files {
        if !matches!(
            media.kind,
            MediaKind::Audio | MediaKind::Video | MediaKind::VideoWithAudio
        ) {
            return Err(unsupported("only audio and video files are joined"));
        }
        let duration = media
            .duration_secs
            .filter(|d| d.is_finite() && *d > 0.0)
            .ok_or_else(|| unsupported("the length of a file is unknown"))?;
        let video = media.primary_video().filter(|_| video_target);
        if video_target && video.is_none() {
            return Err(unsupported(
                "every file joined into a video needs a picture",
            ));
        }
        let audio = media.primary_audio();
        if !video_target && audio.is_none() {
            return Err(unsupported("every file joined into audio needs sound"));
        }
        joined.push((media, video, audio, duration));
    }

    // The same streams everywhere: copied one after the other.
    let (_, first_video, first_audio, _) = joined[0];
    let same = joined.iter().all(|(_, video, audio, _)| {
        let video = match (first_video, video) {
            (Some(a), Some(b)) => a.index == b.index && same_video(a, b),
            (None, None) => true,
            _ => false,
        };
        let audio = match (first_audio, audio) {
            (Some(a), Some(b)) => a.index == b.index && same_audio(a, b),
            (None, None) => true,
            _ => false,
        };
        video && audio
    });
    let adv = &request.advanced;
    // A new loudness: every file is encoded (each one gets it on its own).
    let volume = request.media.as_ref().and_then(|m| m.volume);
    crate::volume::check(volume)?;
    if volume.is_some() && first_audio.is_none() && joined.iter().all(|(_, _, a, _)| a.is_none()) {
        return Err(unsupported("the result has no audio"));
    }
    let copyable = first_video.is_none_or(|v| spec.copy_video.contains(&v.codec.as_str()))
        && first_audio.is_none_or(|a| spec.copy_audio.contains(&a.codec.as_str()))
        && !adv.touches_video()
        && !adv.touches_audio()
        && request.preset != QualityPreset::Small
        && volume.is_none();
    let copy = same && copyable;

    let (video, size, fps) = match first_video {
        Some(v) if copy => (
            Some(VideoPlan::Copy {
                codec: v.codec.clone(),
            }),
            None,
            None,
        ),
        Some(v) => match plan_video(spec, v, request, true) {
            VideoPlan::Encode(mut enc) => {
                // The filter graph scales and paces every file itself.
                let size = enc.width.zip(enc.height).unwrap_or((1280, 720));
                let fps = adv
                    .fps
                    .or(v.fps)
                    .filter(|f| *f > 0.0)
                    .unwrap_or(DEFAULT_FPS);
                enc.width = Some(size.0);
                enc.height = Some(size.1);
                enc.resize = false;
                enc.fps = None;
                (Some(VideoPlan::Encode(enc)), Some(size), Some(fps))
            }
            copied => (Some(copied), None, None),
        },
        None => (None, None, None),
    };
    // The sound of the first file that has some sets the codec.
    let source = joined.iter().find_map(|(_, _, audio, _)| *audio);
    let audio = match source {
        Some(a) if copy => Some(AudioPlan::Copy {
            codec: a.codec.clone(),
        }),
        Some(a) => Some(match plan_audio(spec, a, request, true)? {
            AudioPlan::Encode(mut enc) => {
                enc.sample_rate.get_or_insert(48_000);
                if adv.channels.is_none() {
                    enc.channels = Some(2);
                }
                AudioPlan::Encode(enc)
            }
            copied => copied,
        }),
        None => None,
    };

    let mut plan = ConversionPlan {
        target: request.target,
        video_stream: first_video.map(|v| v.index),
        video,
        audio_stream: first_audio.map(|a| a.index),
        audio,
        cover_art_stream: None,
        image: None,
        frames: None,
        document: None,
        remove_location: request.remove_location && files.iter().any(|m| m.has_location),
        compress: None,
        tracks: Vec::new(),
        trim: None,
        added_audio: None,
        volume,
        loudness: Vec::new(),
        join: Some(JoinPlan {
            copy,
            files: joined
                .iter()
                .map(|(_, video, audio, duration)| JoinedFile {
                    video_stream: video.map(|v| v.index),
                    audio_stream: audio.map(|a| a.index),
                    duration_secs: *duration,
                })
                .collect(),
            width: size.map(|s| s.0),
            height: size.map(|s| s.1),
            fps,
        }),
        extract: Vec::new(),
        warnings: Vec::new(),
        estimate: None,
    };
    if !same {
        plan.warnings.push(ConversionWarning::JoinReencoded);
    }
    if matches!(volume, Some(crate::request::Volume::Gain { db }) if db > 0.0) {
        plan.warnings.push(ConversionWarning::VolumeLimited);
    }
    // Each file as it is written, added up.
    plan.estimate = joined
        .iter()
        .map(|(media, _, _, _)| estimate_size(media, &plan))
        .try_fold((0u64, false), |(bytes, approx), e| {
            e.map(|e| (bytes + e.bytes, approx || e.approximate))
        })
        .map(|(bytes, approximate)| crate::estimate::SizeEstimate { bytes, approximate });
    Ok(plan)
}

/// The list read by FFmpeg's concat demuxer: every file, quoted.
pub fn concat_list(paths: &[std::path::PathBuf]) -> String {
    let mut list = String::from("ffconcat version 1.0\n");
    for path in paths {
        let quoted = path.to_string_lossy().replace('\'', r"'\''");
        list.push_str(&format!("file '{quoted}'\n"));
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::TargetFormat;
    use crate::probe::MediaKind;

    fn video(codec: &str, width: u32, height: u32, fps: f64, audio: Option<&str>) -> MediaInfo {
        MediaInfo {
            kind: if audio.is_some() {
                MediaKind::VideoWithAudio
            } else {
                MediaKind::Video
            },
            duration_secs: Some(10.0),
            video: vec![VideoStream {
                index: 0,
                codec: codec.into(),
                width: Some(width),
                height: Some(height),
                fps: Some(fps),
                bit_rate: Some(4_000_000),
                pix_fmt: Some("yuv420p".into()),
            }],
            audio: audio
                .map(|codec| AudioStream {
                    index: 1,
                    codec: codec.into(),
                    sample_rate: Some(48_000),
                    channels: Some(2),
                    bit_rate: Some(128_000),
                    bit_depth: None,
                })
                .into_iter()
                .collect(),
            container: "mov,mp4,m4a,3gp,3g2,mj2".into(),
            size_bytes: Some(5_000_000),
            bit_rate: None,
            cover_art: None,
            subtitle_streams: 0,
            tracks: Vec::new(),
            picture: None,
            has_tags: false,
            has_location: false,
            document: None,
            text: None,
        }
    }

    fn request(target: TargetFormat) -> ConversionRequest {
        ConversionRequest::new(target, QualityPreset::Balanced)
    }

    #[test]
    fn alike_files_are_copied_one_after_the_other() {
        let a = video("h264", 1920, 1080, 30.0, Some("aac"));
        let plan = plan(&[a.clone(), a.clone()], &request(TargetFormat::Mp4)).unwrap();
        let join = plan.join.as_ref().unwrap();
        assert!(join.copy);
        assert!(plan.is_remux());
        assert!(plan.warnings.is_empty());
        assert_eq!(join.files.len(), 2);
        // Twice the size of one file, roughly.
        let bytes = plan.estimate.unwrap().bytes;
        assert!((8_000_000..12_000_000).contains(&bytes), "{bytes}");
    }

    #[test]
    fn different_files_are_encoded_like_the_first() {
        let a = video("h264", 1280, 720, 25.0, Some("aac"));
        let b = video("vp9", 1920, 1080, 30.0, None);
        let plan = plan(&[a, b], &request(TargetFormat::Mp4)).unwrap();
        let join = plan.join.as_ref().unwrap();
        assert!(!join.copy);
        assert_eq!(
            (join.width, join.height, join.fps),
            (Some(1280), Some(720), Some(25.0))
        );
        assert_eq!(join.files[1].audio_stream, None);
        assert!(plan.warnings.contains(&ConversionWarning::JoinReencoded));
        match &plan.audio {
            Some(AudioPlan::Encode(enc)) => {
                assert_eq!((enc.sample_rate, enc.channels), (Some(48_000), Some(2)))
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&plan.video, Some(VideoPlan::Encode(e)) if !e.resize && e.fps.is_none()));
    }

    #[test]
    fn only_sound_goes_into_audio_formats() {
        let a = video("h264", 1280, 720, 25.0, Some("aac"));
        let b = video("h264", 1280, 720, 25.0, Some("mp3"));
        let plan = plan(&[a.clone(), b], &request(TargetFormat::Mp3)).unwrap();
        assert!(plan.video.is_none());
        assert!(matches!(plan.audio, Some(AudioPlan::Encode(_))));
        // A video without sound has nothing to give.
        let silent = video("h264", 1280, 720, 25.0, None);
        assert!(super::plan(&[a.clone(), silent], &request(TargetFormat::Mp3)).is_err());
        // One file is not a join; pictures and animations are not joined.
        assert!(super::plan(std::slice::from_ref(&a), &request(TargetFormat::Mp4)).is_err());
        assert!(super::plan(&[a.clone(), a.clone()], &request(TargetFormat::Gif)).is_err());
        assert!(super::plan(&[a.clone(), a], &request(TargetFormat::Png)).is_err());
    }

    #[test]
    fn a_new_loudness_encodes_every_file_on_its_own() {
        use crate::request::{MediaEdit, Volume};
        let a = video("h264", 1280, 720, 25.0, Some("aac"));
        let with = |volume| ConversionRequest {
            media: Some(MediaEdit {
                volume: Some(volume),
                ..MediaEdit::default()
            }),
            ..request(TargetFormat::Mp4)
        };
        let plan = plan(&[a.clone(), a.clone()], &with(Volume::Gain { db: -3.0 })).unwrap();
        assert!(!plan.join.as_ref().unwrap().copy);
        // Alike files: no warning that they differ.
        assert!(plan.warnings.is_empty());
        let join = plan.join.clone().unwrap();
        let args = crate::command::join_args(
            &plan,
            &join,
            &["a.mp4".into(), "b.mp4".into()],
            "l".as_ref(),
            "o.mp4".as_ref(),
        )
        .args_lossy()
        .join(" ");
        assert!(args.contains("[0:1]volume=-3dB,aresample=48000"), "{args}");
        assert!(args.contains("[1:1]volume=-3dB,aresample=48000"), "{args}");
        assert!(super::plan(&[a.clone(), a], &with(Volume::Gain { db: 30.0 })).is_err());
    }

    #[test]
    fn the_list_quotes_every_path() {
        let list = concat_list(&["C:\\Film\\l'estate.mp4".into(), "b.mp4".into()]);
        assert_eq!(
            list,
            "ffconcat version 1.0\nfile 'C:\\Film\\l'\\''estate.mp4'\nfile 'b.mp4'\n"
        );
    }
}
