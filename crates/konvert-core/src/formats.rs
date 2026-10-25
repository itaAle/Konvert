//! Declarative catalog of output formats and of the conversions Konvert proposes.
//!
//! Everything format-specific lives in two tables:
//! * [`FORMATS`]: one [`FormatSpec`] per output format (extension, muxer, allowed
//!   codecs, which source codecs can be copied without re-encoding, ...);
//! * [`RULES`]: for each kind of input, which output formats make sense.
//!
//! Adding a format means adding a [`TargetFormat`] variant, one row to
//! [`FORMATS`] and listing it in [`RULES`]; the unit tests check consistency.

use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::probe::{MediaInfo, MediaKind};

/// Output formats Konvert can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TargetFormat {
    Mp3,
    M4a,
    Wav,
    Flac,
    Ogg,
    Opus,
    Wma,
    Aiff,
    Mp4,
    Mkv,
    Mov,
    Webm,
    Avi,
    Gif,
}

impl TargetFormat {
    /// Parses an extension-like identifier (`"mp3"`, `"AIF"`, `"m4v"`).
    pub fn from_id(id: &str) -> Option<Self> {
        let id = id.trim_start_matches('.').to_ascii_lowercase();
        FORMATS
            .iter()
            .find(|s| s.input_extensions.contains(&id.as_str()))
            .map(|s| s.format)
    }
}

/// Whether a format stores audio only or video (possibly with audio).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FormatKind {
    Audio,
    Video,
}

/// Audio encoders Konvert uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AudioCodec {
    /// MP3 (LAME)
    Mp3,
    /// AAC (FFmpeg native encoder)
    Aac,
    /// Uncompressed PCM (WAV, AIFF)
    Pcm,
    Flac,
    /// Vorbis (libvorbis)
    Vorbis,
    /// Opus (libopus)
    Opus,
    /// Windows Media Audio 2
    Wma,
}

/// Video encoders Konvert uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum VideoCodec {
    /// H.264 / AVC (OpenH264, BSD-licensed)
    H264,
    /// AV1 (SVT-AV1)
    Av1,
    /// VP9 (libvpx)
    Vp9,
    /// MPEG-4 Part 2 (FFmpeg native, Xvid compatible)
    Mpeg4,
    /// GIF with an optimized palette
    Gif,
}

impl AudioCodec {
    /// `true` for codecs that do not lose information.
    pub fn is_lossless(self) -> bool {
        matches!(self, AudioCodec::Pcm | AudioCodec::Flac)
    }

    /// Highest channel count the encoder accepts (`None`: no practical limit).
    pub fn max_channels(self) -> Option<u32> {
        match self {
            AudioCodec::Mp3 | AudioCodec::Wma => Some(2),
            _ => None,
        }
    }

    /// Sample rates the encoder accepts (`None`: any common rate).
    pub fn supported_sample_rates(self) -> Option<&'static [u32]> {
        match self {
            AudioCodec::Mp3 => {
                Some(&[8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000])
            }
            AudioCodec::Opus => Some(&[8000, 12000, 16000, 24000, 48000]),
            AudioCodec::Wma => Some(&[8000, 11025, 16000, 22050, 32000, 44100, 48000]),
            _ => None,
        }
    }
}

/// Everything Konvert needs to know about one output format.
#[derive(Debug)]
pub struct FormatSpec {
    pub format: TargetFormat,
    /// Name shown in the UI.
    pub label: &'static str,
    /// Extension of the produced file.
    pub extension: &'static str,
    /// Extensions recognised as this format on input (same-format detection).
    pub input_extensions: &'static [&'static str],
    /// FFmpeg muxer, passed explicitly with `-f`.
    pub muxer: &'static str,
    pub kind: FormatKind,
    /// Allowed video encoders; the first one is the default.
    pub video_codecs: &'static [VideoCodec],
    /// Allowed audio encoders; the first one is the default. Empty: no audio (GIF).
    pub audio_codecs: &'static [AudioCodec],
    /// Source video codecs (FFmpeg names) that can be copied without re-encoding.
    pub copy_video: &'static [&'static str],
    /// Source audio codecs (FFmpeg names) that can be copied without re-encoding.
    pub copy_audio: &'static [&'static str],
    /// The format can embed a cover picture.
    pub cover_art: bool,
    /// Extra muxer options.
    pub muxer_args: &'static [&'static str],
}

impl FormatSpec {
    pub fn default_audio_codec(&self) -> Option<AudioCodec> {
        self.audio_codecs.first().copied()
    }

    pub fn default_video_codec(&self) -> Option<VideoCodec> {
        self.video_codecs.first().copied()
    }

    /// Lossless output (with the default codec).
    pub fn is_lossless(&self) -> bool {
        self.kind == FormatKind::Audio
            && self
                .default_audio_codec()
                .is_some_and(AudioCodec::is_lossless)
    }
}

const MP4_FAMILY_VIDEO_COPY: &[&str] = &["h264", "hevc", "av1", "mpeg4"];
const MP4_FAMILY_AUDIO_COPY: &[&str] = &["aac", "mp3", "alac", "ac3", "eac3"];

/// The format catalog.
pub static FORMATS: &[FormatSpec] = &[
    // --- Audio ---------------------------------------------------------------
    FormatSpec {
        format: TargetFormat::Mp3,
        label: "MP3",
        extension: "mp3",
        input_extensions: &["mp3"],
        muxer: "mp3",
        kind: FormatKind::Audio,
        video_codecs: &[],
        audio_codecs: &[AudioCodec::Mp3],
        copy_video: &[],
        copy_audio: &["mp3"],
        cover_art: true,
        muxer_args: &["-id3v2_version", "3"],
    },
    FormatSpec {
        format: TargetFormat::M4a,
        label: "M4A (AAC)",
        extension: "m4a",
        input_extensions: &["m4a", "m4b"],
        muxer: "ipod",
        kind: FormatKind::Audio,
        video_codecs: &[],
        audio_codecs: &[AudioCodec::Aac],
        copy_video: &[],
        copy_audio: &["aac", "alac"],
        cover_art: true,
        muxer_args: &["-movflags", "+faststart"],
    },
    FormatSpec {
        format: TargetFormat::Wav,
        label: "WAV",
        extension: "wav",
        input_extensions: &["wav", "wave"],
        muxer: "wav",
        kind: FormatKind::Audio,
        video_codecs: &[],
        audio_codecs: &[AudioCodec::Pcm],
        copy_video: &[],
        copy_audio: &[],
        cover_art: false,
        // RF64 header only when the file exceeds 4 GB.
        muxer_args: &["-rf64", "auto"],
    },
    FormatSpec {
        format: TargetFormat::Flac,
        label: "FLAC",
        extension: "flac",
        input_extensions: &["flac"],
        muxer: "flac",
        kind: FormatKind::Audio,
        video_codecs: &[],
        audio_codecs: &[AudioCodec::Flac],
        copy_video: &[],
        copy_audio: &[],
        cover_art: true,
        muxer_args: &[],
    },
    FormatSpec {
        format: TargetFormat::Ogg,
        label: "OGG (Vorbis)",
        extension: "ogg",
        input_extensions: &["ogg", "oga"],
        muxer: "ogg",
        kind: FormatKind::Audio,
        video_codecs: &[],
        audio_codecs: &[AudioCodec::Vorbis],
        copy_video: &[],
        copy_audio: &["vorbis"],
        cover_art: false,
        muxer_args: &[],
    },
    FormatSpec {
        format: TargetFormat::Opus,
        label: "OPUS",
        extension: "opus",
        input_extensions: &["opus"],
        muxer: "opus",
        kind: FormatKind::Audio,
        video_codecs: &[],
        audio_codecs: &[AudioCodec::Opus],
        copy_video: &[],
        copy_audio: &["opus"],
        cover_art: false,
        muxer_args: &[],
    },
    FormatSpec {
        format: TargetFormat::Wma,
        label: "WMA",
        extension: "wma",
        input_extensions: &["wma"],
        muxer: "asf",
        kind: FormatKind::Audio,
        video_codecs: &[],
        audio_codecs: &[AudioCodec::Wma],
        copy_video: &[],
        copy_audio: &["wmav2"],
        cover_art: false,
        muxer_args: &[],
    },
    FormatSpec {
        format: TargetFormat::Aiff,
        label: "AIFF",
        extension: "aiff",
        input_extensions: &["aif", "aiff", "aifc"],
        muxer: "aiff",
        kind: FormatKind::Audio,
        video_codecs: &[],
        audio_codecs: &[AudioCodec::Pcm],
        copy_video: &[],
        copy_audio: &[],
        cover_art: false,
        muxer_args: &[],
    },
    // --- Video ---------------------------------------------------------------
    FormatSpec {
        format: TargetFormat::Mp4,
        label: "MP4",
        extension: "mp4",
        input_extensions: &["mp4", "m4v"],
        muxer: "mp4",
        kind: FormatKind::Video,
        video_codecs: &[VideoCodec::H264, VideoCodec::Av1],
        audio_codecs: &[AudioCodec::Aac, AudioCodec::Mp3],
        copy_video: MP4_FAMILY_VIDEO_COPY,
        copy_audio: MP4_FAMILY_AUDIO_COPY,
        cover_art: false,
        muxer_args: &["-movflags", "+faststart"],
    },
    FormatSpec {
        format: TargetFormat::Mkv,
        label: "MKV",
        extension: "mkv",
        input_extensions: &["mkv"],
        muxer: "matroska",
        kind: FormatKind::Video,
        video_codecs: &[VideoCodec::H264, VideoCodec::Av1, VideoCodec::Vp9],
        audio_codecs: &[
            AudioCodec::Aac,
            AudioCodec::Opus,
            AudioCodec::Flac,
            AudioCodec::Mp3,
        ],
        copy_video: &[
            "h264",
            "hevc",
            "av1",
            "vp8",
            "vp9",
            "mpeg4",
            "mpeg2video",
            "theora",
            "prores",
        ],
        copy_audio: &[
            "aac", "mp3", "opus", "vorbis", "flac", "ac3", "eac3", "dts", "alac", "truehd",
        ],
        cover_art: false,
        muxer_args: &[],
    },
    FormatSpec {
        format: TargetFormat::Mov,
        label: "MOV",
        extension: "mov",
        input_extensions: &["mov", "qt"],
        muxer: "mov",
        kind: FormatKind::Video,
        video_codecs: &[VideoCodec::H264],
        audio_codecs: &[AudioCodec::Aac],
        copy_video: &["h264", "hevc", "prores", "mpeg4", "mjpeg"],
        copy_audio: &["aac", "alac", "mp3", "ac3", "pcm_s16le", "pcm_s24le"],
        cover_art: false,
        muxer_args: &["-movflags", "+faststart"],
    },
    FormatSpec {
        format: TargetFormat::Webm,
        label: "WebM",
        extension: "webm",
        input_extensions: &["webm"],
        muxer: "webm",
        kind: FormatKind::Video,
        video_codecs: &[VideoCodec::Vp9, VideoCodec::Av1],
        audio_codecs: &[AudioCodec::Opus, AudioCodec::Vorbis],
        copy_video: &["vp8", "vp9", "av1"],
        copy_audio: &["opus", "vorbis"],
        cover_art: false,
        muxer_args: &[],
    },
    FormatSpec {
        format: TargetFormat::Avi,
        label: "AVI",
        extension: "avi",
        input_extensions: &["avi"],
        muxer: "avi",
        kind: FormatKind::Video,
        video_codecs: &[VideoCodec::Mpeg4],
        audio_codecs: &[AudioCodec::Mp3],
        copy_video: &["mpeg4", "mjpeg"],
        copy_audio: &["mp3", "ac3", "pcm_s16le"],
        cover_art: false,
        muxer_args: &[],
    },
    FormatSpec {
        format: TargetFormat::Gif,
        label: "GIF",
        extension: "gif",
        input_extensions: &["gif"],
        muxer: "gif",
        kind: FormatKind::Video,
        video_codecs: &[VideoCodec::Gif],
        audio_codecs: &[],
        copy_video: &[],
        copy_audio: &[],
        cover_art: false,
        muxer_args: &["-loop", "0"],
    },
];

/// Catalog entry of a format.
pub fn spec(format: TargetFormat) -> &'static FormatSpec {
    FORMATS
        .iter()
        .find(|s| s.format == format)
        .expect("every TargetFormat has a FormatSpec (checked by tests)")
}

const AUDIO_TARGETS: &[TargetFormat] = &[
    TargetFormat::Mp3,
    TargetFormat::M4a,
    TargetFormat::Wav,
    TargetFormat::Flac,
    TargetFormat::Ogg,
    TargetFormat::Opus,
    TargetFormat::Wma,
    TargetFormat::Aiff,
];

const VIDEO_TARGETS: &[TargetFormat] = &[
    TargetFormat::Mp4,
    TargetFormat::Mkv,
    TargetFormat::Mov,
    TargetFormat::Webm,
    TargetFormat::Avi,
    TargetFormat::Gif,
];

/// What Konvert proposes for one kind of input.
#[derive(Debug)]
pub struct Rule {
    pub input: MediaKind,
    /// Video outputs.
    pub video: &'static [TargetFormat],
    /// Audio outputs (for video inputs: "audio only" extraction).
    pub audio: &'static [TargetFormat],
}

/// Proposal rules. Audio inputs never get video outputs.
pub static RULES: &[Rule] = &[
    Rule {
        input: MediaKind::Audio,
        video: &[],
        audio: AUDIO_TARGETS,
    },
    Rule {
        input: MediaKind::Video,
        video: VIDEO_TARGETS,
        audio: &[],
    },
    Rule {
        input: MediaKind::VideoWithAudio,
        video: VIDEO_TARGETS,
        audio: AUDIO_TARGETS,
    },
];

/// How a proposed target relates to the input, for grouping in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TargetGroup {
    /// Audio → audio.
    Audio,
    /// Video → video.
    Video,
    /// Video → audio only (extracts the soundtrack).
    AudioOnly,
}

/// A conversion proposed to the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TargetOption {
    pub format: TargetFormat,
    pub label: String,
    pub extension: String,
    pub group: TargetGroup,
    /// The output keeps all the information of the decoded input.
    pub lossless: bool,
    /// Same format as the input (useful to change quality or size).
    pub same_as_input: bool,
}

/// Format of an input file, guessed from its extension.
pub fn source_format(input: &Path) -> Option<TargetFormat> {
    let ext = input.extension()?.to_str()?.to_ascii_lowercase();
    FORMATS
        .iter()
        .find(|s| s.input_extensions.contains(&ext.as_str()))
        .map(|s| s.format)
}

/// Whether `format` is proposed for this input.
pub fn is_offered(media: &MediaInfo, input: &Path, format: TargetFormat) -> bool {
    available_targets(media, input)
        .iter()
        .any(|t| t.format == format)
}

/// Output formats that make sense for this input, in display order.
pub fn available_targets(media: &MediaInfo, input: &Path) -> Vec<TargetOption> {
    let Some(rule) = RULES.iter().find(|r| r.input == media.kind) else {
        return Vec::new();
    };
    let source = source_format(input);
    let audio_group = if rule.video.is_empty() {
        TargetGroup::Audio
    } else {
        TargetGroup::AudioOnly
    };
    let groups = [(rule.video, TargetGroup::Video), (rule.audio, audio_group)];

    groups
        .into_iter()
        .flat_map(|(formats, group)| formats.iter().map(move |f| (spec(*f), group)))
        .filter_map(|(spec, group)| {
            let same_as_input = source == Some(spec.format);
            // Re-encoding into the same lossless format (or GIF into GIF) gains nothing.
            let pointless =
                same_as_input && (spec.is_lossless() || spec.format == TargetFormat::Gif);
            (!pointless).then(|| TargetOption {
                format: spec.format,
                label: spec.label.to_owned(),
                extension: spec.extension.to_owned(),
                group,
                lossless: spec.is_lossless(),
                same_as_input,
            })
        })
        .collect()
}

/// `true` for lossless audio codecs (FFmpeg names).
pub fn is_lossless_audio_codec(codec: &str) -> bool {
    codec.starts_with("pcm_")
        || matches!(
            codec,
            "flac"
                | "alac"
                | "wavpack"
                | "ape"
                | "tta"
                | "mlp"
                | "truehd"
                | "mp4als"
                | "wmalossless"
                | "shorten"
        )
}

/// `true` for lossless video codecs (FFmpeg names).
pub fn is_lossless_video_codec(codec: &str) -> bool {
    matches!(
        codec,
        "ffv1"
            | "huffyuv"
            | "ffvhuff"
            | "utvideo"
            | "rawvideo"
            | "png"
            | "apng"
            | "qtrle"
            | "magicyuv"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::parse_ffprobe_json;
    use std::collections::HashSet;

    const ALL: &[TargetFormat] = &[
        TargetFormat::Mp3,
        TargetFormat::M4a,
        TargetFormat::Wav,
        TargetFormat::Flac,
        TargetFormat::Ogg,
        TargetFormat::Opus,
        TargetFormat::Wma,
        TargetFormat::Aiff,
        TargetFormat::Mp4,
        TargetFormat::Mkv,
        TargetFormat::Mov,
        TargetFormat::Webm,
        TargetFormat::Avi,
        TargetFormat::Gif,
    ];

    fn media(json: &str) -> MediaInfo {
        parse_ffprobe_json(json.as_bytes()).unwrap()
    }

    fn audio() -> MediaInfo {
        media(
            r#"{ "streams": [{ "index": 0, "codec_name": "mp3", "codec_type": "audio" }],
                   "format": { "format_name": "mp3" } }"#,
        )
    }

    fn video_with_audio() -> MediaInfo {
        media(
            r#"{ "streams": [
            { "index": 0, "codec_name": "h264", "codec_type": "video", "width": 640, "height": 360 },
            { "index": 1, "codec_name": "aac", "codec_type": "audio" }],
            "format": { "format_name": "mov,mp4,m4a,3gp,3g2,mj2" } }"#,
        )
    }

    fn silent_video() -> MediaInfo {
        media(
            r#"{ "streams": [{ "index": 0, "codec_name": "gif", "codec_type": "video",
            "width": 100, "height": 100 }], "format": { "format_name": "gif" } }"#,
        )
    }

    fn formats(options: &[TargetOption]) -> Vec<TargetFormat> {
        options.iter().map(|o| o.format).collect()
    }

    #[test]
    fn catalog_is_complete_and_consistent() {
        assert_eq!(FORMATS.len(), ALL.len());
        let mut extensions = HashSet::new();
        for f in ALL {
            let s = spec(*f);
            assert!(
                extensions.insert(s.extension),
                "duplicate extension {}",
                s.extension
            );
            assert!(s.input_extensions.contains(&s.extension), "{:?}", f);
            match s.kind {
                FormatKind::Audio => {
                    assert!(
                        s.video_codecs.is_empty() && !s.audio_codecs.is_empty(),
                        "{f:?}"
                    );
                }
                FormatKind::Video => assert!(!s.video_codecs.is_empty(), "{f:?}"),
            }
        }
    }

    #[test]
    fn every_format_is_reachable_from_some_input() {
        let offered: HashSet<TargetFormat> = RULES
            .iter()
            .flat_map(|r| r.video.iter().chain(r.audio))
            .copied()
            .collect();
        assert_eq!(offered.len(), ALL.len());
    }

    #[test]
    fn audio_input_gets_only_audio_formats() {
        let options = available_targets(&audio(), Path::new("song.flac"));
        assert!(
            options
                .iter()
                .all(|o| spec(o.format).kind == FormatKind::Audio)
        );
        assert!(options.iter().all(|o| o.group == TargetGroup::Audio));
        // FLAC → FLAC is pointless, every other audio format is proposed.
        assert_eq!(
            formats(&options),
            [
                TargetFormat::Mp3,
                TargetFormat::M4a,
                TargetFormat::Wav,
                TargetFormat::Ogg,
                TargetFormat::Opus,
                TargetFormat::Wma,
                TargetFormat::Aiff
            ]
        );
    }

    #[test]
    fn lossy_same_format_is_kept_and_flagged() {
        let options = available_targets(&audio(), Path::new("song.MP3"));
        let mp3 = options
            .iter()
            .find(|o| o.format == TargetFormat::Mp3)
            .unwrap();
        assert!(mp3.same_as_input);
        assert!(!mp3.lossless);
        assert!(
            options
                .iter()
                .find(|o| o.format == TargetFormat::Wav)
                .unwrap()
                .lossless
        );
    }

    #[test]
    fn video_with_audio_gets_video_formats_and_audio_extraction() {
        let options = available_targets(&video_with_audio(), Path::new("clip.mp4"));
        let video: Vec<_> = options
            .iter()
            .filter(|o| o.group == TargetGroup::Video)
            .collect();
        let audio_only: Vec<_> = options
            .iter()
            .filter(|o| o.group == TargetGroup::AudioOnly)
            .collect();
        assert_eq!(video.len(), VIDEO_TARGETS.len());
        assert_eq!(audio_only.len(), AUDIO_TARGETS.len());
        assert!(
            video
                .iter()
                .any(|o| o.format == TargetFormat::Mp4 && o.same_as_input)
        );
        // Video formats come first.
        assert_eq!(options[0].group, TargetGroup::Video);
    }

    #[test]
    fn silent_video_gets_no_audio_formats_and_gif_is_not_proposed_for_gif() {
        let options = available_targets(&silent_video(), Path::new("anim.gif"));
        assert!(options.iter().all(|o| o.group == TargetGroup::Video));
        assert!(!formats(&options).contains(&TargetFormat::Gif));
        assert!(formats(&options).contains(&TargetFormat::Mp4));
    }

    #[test]
    fn source_format_uses_the_extension() {
        assert_eq!(
            source_format(Path::new("a/b/Song.Flac")),
            Some(TargetFormat::Flac)
        );
        assert_eq!(source_format(Path::new("x.aif")), Some(TargetFormat::Aiff));
        assert_eq!(source_format(Path::new("x.m4v")), Some(TargetFormat::Mp4));
        assert_eq!(source_format(Path::new("x.ts")), None);
        assert_eq!(source_format(Path::new("noext")), None);
    }

    #[test]
    fn lossless_codec_detection() {
        for c in ["pcm_s16le", "pcm_f32be", "flac", "alac", "wavpack"] {
            assert!(is_lossless_audio_codec(c), "{c}");
        }
        for c in ["mp3", "aac", "opus", "vorbis", "wmav2", "ac3"] {
            assert!(!is_lossless_audio_codec(c), "{c}");
        }
        assert!(is_lossless_video_codec("ffv1"));
        assert!(!is_lossless_video_codec("h264"));
    }
}
