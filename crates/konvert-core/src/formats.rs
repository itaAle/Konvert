//! Declarative catalog of output formats and of the conversions Konvert proposes.
//!
//! Everything format-specific lives in two tables:
//! * [`FORMATS`]: one [`FormatSpec`] per output format (extension, muxer, allowed
//!   codecs, which source codecs can be copied without re-encoding, ...);
//! * [`RULES`]: for each kind of input, which output formats make sense.
//!
//! Pictures (JPG, PNG, WebP, AVIF) are formats of [`FormatKind::Image`] with a
//! single [`ImageCodec`]; they are only proposed for still pictures.
//!
//! Adding a format means adding a [`TargetFormat`] variant, one row to
//! [`FORMATS`] and listing it in [`RULES`]; the unit tests check consistency.
//!
//! A few formats share an extension (ALAC and AAC are both `.m4a`): the
//! identifier of a format is its serialized [`TargetFormat`] name, and the
//! same-format detection also looks at the kind of input and its codec.

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
    /// Apple Lossless in an M4A file.
    Alac,
    Wav,
    Flac,
    Ogg,
    Opus,
    Wma,
    Aiff,
    /// Raw AAC (ADTS).
    Aac,
    Ac3,
    Eac3,
    Mp2,
    /// Apple Core Audio Format (ALAC or PCM).
    Caf,
    /// Matroska audio.
    Mka,
    Mp4,
    Mkv,
    Mov,
    Webm,
    Avi,
    /// MPEG transport stream.
    Ts,
    /// MPEG-2 program stream.
    Mpg,
    #[serde(rename = "3gp")]
    ThreeGp,
    Flv,
    Gif,
    AnimatedWebp,
    Apng,
    Jpg,
    Png,
    Webp,
    Avif,
    Bmp,
    Tiff,
    StillGif,
    /// Windows icon (16, 32, 48 and 256 px).
    Ico,
}

impl TargetFormat {
    /// Stable identifier, as serialized for the UI (`"mp3"`, `"alac"`).
    pub fn id(self) -> String {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::String(id)) => id,
            _ => unreachable!("unit variants serialize to strings"),
        }
    }

    /// Parses an identifier (`"alac"`) or an extension (`"mp3"`, `"AIF"`, `"m4v"`).
    pub fn from_id(id: &str) -> Option<Self> {
        let id = id.trim_start_matches('.').to_ascii_lowercase();
        FORMATS
            .iter()
            .find(|s| s.format.id().eq_ignore_ascii_case(&id))
            .or_else(|| {
                FORMATS
                    .iter()
                    .find(|s| s.input_extensions.contains(&id.as_str()))
            })
            .map(|s| s.format)
    }
}

/// Whether a format stores audio only, video (possibly with audio) or a picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FormatKind {
    Audio,
    Video,
    Image,
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
    /// Apple Lossless (FFmpeg native encoder)
    Alac,
    /// Dolby Digital (FFmpeg native encoder)
    Ac3,
    /// Dolby Digital Plus (FFmpeg native encoder)
    Eac3,
    /// MPEG-1 Audio Layer II (FFmpeg native encoder)
    Mp2,
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
    /// MPEG-2 (FFmpeg native, DVD compatible)
    Mpeg2,
    /// GIF with an optimized palette
    Gif,
    /// Animated WebP (libwebp)
    Webp,
    /// Animated PNG (FFmpeg native, lossless)
    Apng,
}

impl VideoCodec {
    /// Short looping animations (no audio, limited size and frame rate).
    pub fn is_animation(self) -> bool {
        matches!(self, VideoCodec::Gif | VideoCodec::Webp | VideoCodec::Apng)
    }

    pub fn is_lossless(self) -> bool {
        self == VideoCodec::Apng
    }
}

/// Picture encoders Konvert uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ImageCodec {
    /// JPEG (FFmpeg native encoder)
    Jpeg,
    /// PNG (FFmpeg native encoder, lossless)
    Png,
    /// WebP (libwebp, lossy or lossless)
    Webp,
    /// AVIF: an AV1 still picture (libaom)
    Avif,
    /// Uncompressed 24-bit BMP (FFmpeg native encoder)
    Bmp,
    /// Deflate-compressed TIFF (FFmpeg native encoder)
    Tiff,
    /// Still GIF with an optimized 256-color palette
    Gif,
    /// Windows icon: PNG pictures of several sizes in one file
    Ico,
}

impl ImageCodec {
    /// `true` for codecs that always keep every pixel.
    pub fn is_lossless(self) -> bool {
        matches!(
            self,
            ImageCodec::Png | ImageCodec::Bmp | ImageCodec::Tiff | ImageCodec::Ico
        )
    }

    /// The format can store transparency.
    pub fn supports_alpha(self) -> bool {
        !matches!(self, ImageCodec::Jpeg | ImageCodec::Bmp)
    }

    /// Largest width or height the format can store (`None`: no practical limit).
    pub fn max_dimension(self) -> Option<u32> {
        match self {
            ImageCodec::Gif => Some(65_535),
            ImageCodec::Ico => Some(256),
            ImageCodec::Bmp | ImageCodec::Tiff => None,
            ImageCodec::Jpeg => Some(65_535),
            ImageCodec::Webp => Some(16_383),
            ImageCodec::Avif => Some(65_536),
            ImageCodec::Png => None,
        }
    }
}

impl AudioCodec {
    /// `true` for codecs that do not lose information.
    pub fn is_lossless(self) -> bool {
        matches!(self, AudioCodec::Pcm | AudioCodec::Flac | AudioCodec::Alac)
    }

    /// Highest channel count the encoder accepts (`None`: no practical limit).
    pub fn max_channels(self) -> Option<u32> {
        match self {
            AudioCodec::Mp3 | AudioCodec::Wma | AudioCodec::Mp2 => Some(2),
            // 5.1
            AudioCodec::Ac3 | AudioCodec::Eac3 => Some(6),
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
            AudioCodec::Ac3 | AudioCodec::Eac3 => Some(&[32000, 44100, 48000]),
            AudioCodec::Mp2 => Some(&[16000, 22050, 24000, 32000, 44100, 48000]),
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
    /// The cover is stored as a picture comment (Ogg), not as an attached picture.
    pub cover_art_tag: bool,
    /// The format keeps tags (title, artist, album...).
    pub tags: bool,
    /// Extra muxer options.
    pub muxer_args: &'static [&'static str],
    /// Picture encoder (picture formats only).
    pub image_codec: Option<ImageCodec>,
    /// Less common format, shown under "More formats" in the UI.
    pub secondary: bool,
    /// For formats sharing an extension: source codecs (FFmpeg names) that
    /// identify this format. Empty: any codec.
    pub detect_codecs: &'static [&'static str],
}

/// Fields most formats leave empty (`..SPEC` in the catalog).
const SPEC: FormatSpec = FormatSpec {
    format: TargetFormat::Mp3,
    label: "",
    extension: "",
    input_extensions: &[],
    muxer: "",
    kind: FormatKind::Audio,
    video_codecs: &[],
    audio_codecs: &[],
    copy_video: &[],
    copy_audio: &[],
    cover_art: false,
    cover_art_tag: false,
    tags: true,
    muxer_args: &[],
    image_codec: None,
    secondary: false,
    detect_codecs: &[],
};

impl FormatSpec {
    pub fn default_audio_codec(&self) -> Option<AudioCodec> {
        self.audio_codecs.first().copied()
    }

    pub fn default_video_codec(&self) -> Option<VideoCodec> {
        self.video_codecs.first().copied()
    }

    /// Lossless output (with the default codec).
    pub fn is_lossless(&self) -> bool {
        match self.kind {
            FormatKind::Audio => self
                .default_audio_codec()
                .is_some_and(AudioCodec::is_lossless),
            FormatKind::Image => self.image_codec.is_some_and(ImageCodec::is_lossless),
            FormatKind::Video => self
                .default_video_codec()
                .is_some_and(VideoCodec::is_lossless),
        }
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
        image_codec: None,
        ..SPEC
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
        image_codec: None,
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Alac,
        label: "ALAC (M4A)",
        extension: "m4a",
        input_extensions: &["m4a", "alac"],
        muxer: "ipod",
        audio_codecs: &[AudioCodec::Alac],
        copy_audio: &["alac"],
        cover_art: true,
        muxer_args: &["-movflags", "+faststart"],
        detect_codecs: &["alac"],
        ..SPEC
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
        image_codec: None,
        ..SPEC
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
        image_codec: None,
        ..SPEC
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
        cover_art_tag: true,
        muxer_args: &[],
        image_codec: None,
        ..SPEC
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
        cover_art_tag: true,
        muxer_args: &[],
        image_codec: None,
        ..SPEC
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
        image_codec: None,
        ..SPEC
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
        cover_art: true,
        // ID3 tags, with the cover picture.
        muxer_args: &["-write_id3v2", "1"],
        image_codec: None,
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Aac,
        label: "AAC",
        extension: "aac",
        input_extensions: &["aac"],
        muxer: "adts",
        audio_codecs: &[AudioCodec::Aac],
        copy_audio: &["aac"],
        // ID3 tags before the audio.
        muxer_args: &["-write_id3v2", "1"],
        secondary: true,
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Ac3,
        label: "AC3",
        extension: "ac3",
        input_extensions: &["ac3"],
        muxer: "ac3",
        audio_codecs: &[AudioCodec::Ac3],
        copy_audio: &["ac3"],
        tags: false,
        secondary: true,
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Eac3,
        label: "E-AC3",
        extension: "eac3",
        input_extensions: &["eac3", "ec3"],
        muxer: "eac3",
        audio_codecs: &[AudioCodec::Eac3],
        copy_audio: &["eac3"],
        tags: false,
        secondary: true,
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Mp2,
        label: "MP2",
        extension: "mp2",
        input_extensions: &["mp2"],
        muxer: "mp2",
        audio_codecs: &[AudioCodec::Mp2],
        copy_audio: &["mp2"],
        tags: false,
        secondary: true,
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Caf,
        label: "CAF (ALAC)",
        extension: "caf",
        input_extensions: &["caf"],
        // FFmpeg's CAF muxer takes ALAC and PCM, not AAC or Opus.
        muxer: "caf",
        audio_codecs: &[AudioCodec::Alac, AudioCodec::Pcm],
        copy_audio: &["alac"],
        secondary: true,
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Mka,
        label: "MKA",
        extension: "mka",
        input_extensions: &["mka"],
        muxer: "matroska",
        // The default depends on the source (see `plan.rs`): FLAC or Opus.
        audio_codecs: &[
            AudioCodec::Opus,
            AudioCodec::Flac,
            AudioCodec::Aac,
            AudioCodec::Mp3,
            AudioCodec::Vorbis,
        ],
        // Matroska stores almost any audio codec: the soundtrack is usually copied.
        copy_audio: &[
            "aac",
            "mp3",
            "mp2",
            "opus",
            "vorbis",
            "flac",
            "alac",
            "ac3",
            "eac3",
            "dts",
            "truehd",
            "wavpack",
            "pcm_s16le",
            "pcm_s24le",
            "pcm_s32le",
            "pcm_f32le",
        ],
        secondary: true,
        ..SPEC
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
        image_codec: None,
        ..SPEC
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
        image_codec: None,
        ..SPEC
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
        image_codec: None,
        ..SPEC
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
        image_codec: None,
        ..SPEC
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
        image_codec: None,
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Ts,
        label: "TS",
        extension: "ts",
        input_extensions: &["ts"],
        muxer: "mpegts",
        kind: FormatKind::Video,
        video_codecs: &[VideoCodec::H264],
        audio_codecs: &[AudioCodec::Aac, AudioCodec::Ac3, AudioCodec::Mp2],
        copy_video: &["h264", "hevc", "mpeg2video"],
        copy_audio: &["aac", "mp3", "mp2", "ac3", "eac3"],
        secondary: true,
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Mpg,
        label: "MPG (MPEG-2)",
        extension: "mpg",
        input_extensions: &["mpg", "mpeg"],
        // MPEG-2 program stream, as on DVDs.
        muxer: "vob",
        kind: FormatKind::Video,
        video_codecs: &[VideoCodec::Mpeg2],
        audio_codecs: &[AudioCodec::Mp2, AudioCodec::Ac3],
        copy_video: &["mpeg2video", "mpeg1video"],
        copy_audio: &["mp2", "ac3"],
        secondary: true,
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::ThreeGp,
        label: "3GP",
        extension: "3gp",
        input_extensions: &["3gp"],
        muxer: "3gp",
        kind: FormatKind::Video,
        video_codecs: &[VideoCodec::H264],
        audio_codecs: &[AudioCodec::Aac],
        copy_video: &["h264", "mpeg4", "h263"],
        copy_audio: &["aac", "amr_nb", "amr_wb"],
        muxer_args: &["-movflags", "+faststart"],
        secondary: true,
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Flv,
        label: "FLV",
        extension: "flv",
        input_extensions: &["flv"],
        muxer: "flv",
        kind: FormatKind::Video,
        video_codecs: &[VideoCodec::H264],
        audio_codecs: &[AudioCodec::Aac, AudioCodec::Mp3],
        copy_video: &["h264", "flv1"],
        copy_audio: &["aac", "mp3"],
        secondary: true,
        ..SPEC
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
        image_codec: None,
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::AnimatedWebp,
        label: "WebP",
        extension: "webp",
        input_extensions: &["webp"],
        muxer: "webp",
        kind: FormatKind::Video,
        video_codecs: &[VideoCodec::Webp],
        muxer_args: &["-loop", "0"],
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Apng,
        label: "APNG",
        extension: "apng",
        // Animated PNG files often keep the plain .png extension.
        input_extensions: &["apng", "png"],
        muxer: "apng",
        kind: FormatKind::Video,
        video_codecs: &[VideoCodec::Apng],
        muxer_args: &["-plays", "0"],
        secondary: true,
        ..SPEC
    },
    // --- Pictures ------------------------------------------------------------
    FormatSpec {
        format: TargetFormat::Jpg,
        label: "JPG",
        extension: "jpg",
        input_extensions: &["jpg", "jpeg", "jpe", "jfif"],
        // One encoded picture written as is (no file name patterns).
        muxer: "image2pipe",
        kind: FormatKind::Image,
        video_codecs: &[],
        audio_codecs: &[],
        copy_video: &[],
        copy_audio: &[],
        cover_art: false,
        muxer_args: &[],
        image_codec: Some(ImageCodec::Jpeg),
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Png,
        label: "PNG",
        extension: "png",
        input_extensions: &["png"],
        muxer: "image2pipe",
        kind: FormatKind::Image,
        video_codecs: &[],
        audio_codecs: &[],
        copy_video: &[],
        copy_audio: &[],
        cover_art: false,
        muxer_args: &[],
        image_codec: Some(ImageCodec::Png),
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Webp,
        label: "WebP",
        extension: "webp",
        input_extensions: &["webp"],
        muxer: "webp",
        kind: FormatKind::Image,
        video_codecs: &[],
        audio_codecs: &[],
        copy_video: &[],
        copy_audio: &[],
        cover_art: false,
        muxer_args: &[],
        image_codec: Some(ImageCodec::Webp),
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Avif,
        label: "AVIF",
        extension: "avif",
        input_extensions: &["avif"],
        muxer: "avif",
        kind: FormatKind::Image,
        video_codecs: &[],
        audio_codecs: &[],
        copy_video: &[],
        copy_audio: &[],
        cover_art: false,
        muxer_args: &[],
        image_codec: Some(ImageCodec::Avif),
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Bmp,
        label: "BMP",
        extension: "bmp",
        input_extensions: &["bmp"],
        muxer: "image2pipe",
        kind: FormatKind::Image,
        image_codec: Some(ImageCodec::Bmp),
        secondary: true,
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Tiff,
        label: "TIFF",
        extension: "tiff",
        input_extensions: &["tif", "tiff"],
        muxer: "image2pipe",
        kind: FormatKind::Image,
        image_codec: Some(ImageCodec::Tiff),
        secondary: true,
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::StillGif,
        label: "GIF",
        extension: "gif",
        // GIF files are always read as animations (see `probe.rs`).
        input_extensions: &["gif"],
        muxer: "gif",
        kind: FormatKind::Image,
        image_codec: Some(ImageCodec::Gif),
        secondary: true,
        ..SPEC
    },
    FormatSpec {
        format: TargetFormat::Ico,
        label: "ICO",
        extension: "ico",
        input_extensions: &["ico"],
        muxer: "ico",
        kind: FormatKind::Image,
        image_codec: Some(ImageCodec::Ico),
        secondary: true,
        ..SPEC
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
    TargetFormat::Alac,
    TargetFormat::Wav,
    TargetFormat::Flac,
    TargetFormat::Ogg,
    TargetFormat::Opus,
    TargetFormat::Wma,
    TargetFormat::Aiff,
    TargetFormat::Aac,
    TargetFormat::Ac3,
    TargetFormat::Eac3,
    TargetFormat::Mp2,
    TargetFormat::Caf,
    TargetFormat::Mka,
];

const VIDEO_TARGETS: &[TargetFormat] = &[
    TargetFormat::Mp4,
    TargetFormat::Mkv,
    TargetFormat::Mov,
    TargetFormat::Webm,
    TargetFormat::Avi,
    TargetFormat::Ts,
    TargetFormat::Mpg,
    TargetFormat::ThreeGp,
    TargetFormat::Flv,
    TargetFormat::Gif,
    TargetFormat::AnimatedWebp,
    TargetFormat::Apng,
];

const IMAGE_TARGETS: &[TargetFormat] = &[
    TargetFormat::Jpg,
    TargetFormat::Png,
    TargetFormat::Webp,
    TargetFormat::Avif,
    TargetFormat::Bmp,
    TargetFormat::Tiff,
    TargetFormat::StillGif,
    TargetFormat::Ico,
];

/// Pictures taken from a video (one frame or a series).
const FRAME_TARGETS: &[TargetFormat] = &[TargetFormat::Jpg, TargetFormat::Png, TargetFormat::Webp];

/// What Konvert proposes for one kind of input.
#[derive(Debug)]
pub struct Rule {
    pub input: MediaKind,
    /// Video outputs.
    pub video: &'static [TargetFormat],
    /// Audio outputs (for video inputs: "audio only" extraction).
    pub audio: &'static [TargetFormat],
    /// Picture outputs.
    pub image: &'static [TargetFormat],
}

/// Proposal rules. Audio inputs never get video outputs; pictures only get
/// picture outputs; videos also give pictures (frames).
pub static RULES: &[Rule] = &[
    Rule {
        input: MediaKind::Audio,
        video: &[],
        audio: AUDIO_TARGETS,
        image: &[],
    },
    Rule {
        input: MediaKind::Video,
        video: VIDEO_TARGETS,
        audio: &[],
        image: FRAME_TARGETS,
    },
    Rule {
        input: MediaKind::VideoWithAudio,
        video: VIDEO_TARGETS,
        audio: AUDIO_TARGETS,
        image: FRAME_TARGETS,
    },
    Rule {
        input: MediaKind::Image,
        video: &[],
        audio: &[],
        image: IMAGE_TARGETS,
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
    /// Picture → picture.
    Image,
    /// Video → pictures (one frame or a series).
    Frames,
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
    pub kind: FormatKind,
    /// Less common format, shown under "More formats".
    pub secondary: bool,
    /// Video encoders the user may pick in the advanced options (first = default).
    pub video_codecs: Vec<VideoCodec>,
    /// Audio encoders the user may pick (first = default); empty for GIF.
    pub audio_codecs: Vec<AudioCodecInfo>,
}

/// What the advanced options need to know about an audio encoder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioCodecInfo {
    pub codec: AudioCodec,
    pub lossless: bool,
    /// Accepted sample rates (`None`: any common rate).
    pub sample_rates: Option<Vec<u32>>,
    pub max_channels: Option<u32>,
}

impl AudioCodecInfo {
    pub fn of(codec: AudioCodec) -> Self {
        Self {
            codec,
            lossless: codec.is_lossless(),
            sample_rates: codec.supported_sample_rates().map(<[u32]>::to_vec),
            max_channels: codec.max_channels(),
        }
    }
}

/// Format of an input file: its extension, the kind of content (a `.webp`
/// file may be a picture or an animation) and, for formats sharing an
/// extension, the codec (an `.m4a` file may hold AAC or ALAC).
pub fn source_format(media: &MediaInfo, input: &Path) -> Option<TargetFormat> {
    let ext = input.extension()?.to_str()?.to_ascii_lowercase();
    let codec = media
        .primary_audio()
        .filter(|_| media.kind == MediaKind::Audio)
        .map(|a| a.codec.as_str())
        .or_else(|| media.primary_video().map(|v| v.codec.as_str()))
        .unwrap_or_default();
    let candidates: Vec<&FormatSpec> = FORMATS
        .iter()
        .filter(|s| s.input_extensions.contains(&ext.as_str()))
        .filter(|s| (s.kind == FormatKind::Image) == (media.kind == MediaKind::Image))
        .collect();
    candidates
        .iter()
        .find(|s| s.detect_codecs.contains(&codec))
        .or_else(|| candidates.iter().find(|s| s.detect_codecs.is_empty()))
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
    let source = source_format(media, input);
    let audio_group = if rule.video.is_empty() {
        TargetGroup::Audio
    } else {
        TargetGroup::AudioOnly
    };
    let image_group = if media.kind == MediaKind::Image {
        TargetGroup::Image
    } else {
        TargetGroup::Frames
    };
    let groups = [
        (rule.video, TargetGroup::Video),
        (rule.audio, audio_group),
        (rule.image, image_group),
    ];

    groups
        .into_iter()
        .flat_map(|(formats, group)| formats.iter().map(move |f| (spec(*f), group)))
        .filter_map(|(spec, group)| {
            let same_as_input = source == Some(spec.format);
            // Re-encoding into the same lossless format (or an animation into
            // the same kind of animation) gains nothing.
            let animation = spec
                .default_video_codec()
                .is_some_and(VideoCodec::is_animation);
            let pointless = same_as_input && (spec.is_lossless() || animation);
            (!pointless).then(|| TargetOption {
                format: spec.format,
                label: spec.label.to_owned(),
                extension: spec.extension.to_owned(),
                group,
                lossless: spec.is_lossless(),
                same_as_input,
                kind: spec.kind,
                secondary: spec.secondary,
                video_codecs: spec.video_codecs.to_vec(),
                audio_codecs: spec
                    .audio_codecs
                    .iter()
                    .copied()
                    .map(AudioCodecInfo::of)
                    .collect(),
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

/// `true` for pictures stored without loss (FFmpeg codec and pixel format names).
/// Lossless WebP decodes to RGB, lossy WebP to YUV.
pub fn is_lossless_image(codec: &str, pix_fmt: Option<&str>) -> bool {
    match codec {
        "webp" => pix_fmt.is_some_and(|p| !p.starts_with("yuv")),
        "bmp" | "tiff" | "qoi" | "pam" | "ppm" | "pgm" | "pbm" | "targa" | "sgi" | "pcx"
        | "jpegls" | "xwd" | "sunrast" | "dpx" => true,
        _ => is_lossless_video_codec(codec),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::parse_ffprobe_json;
    use std::collections::HashSet;

    const ALL: &[TargetFormat] = &[
        TargetFormat::Mp3,
        TargetFormat::M4a,
        TargetFormat::Alac,
        TargetFormat::Wav,
        TargetFormat::Flac,
        TargetFormat::Ogg,
        TargetFormat::Opus,
        TargetFormat::Wma,
        TargetFormat::Aiff,
        TargetFormat::Aac,
        TargetFormat::Ac3,
        TargetFormat::Eac3,
        TargetFormat::Mp2,
        TargetFormat::Caf,
        TargetFormat::Mka,
        TargetFormat::Mp4,
        TargetFormat::Mkv,
        TargetFormat::Mov,
        TargetFormat::Webm,
        TargetFormat::Avi,
        TargetFormat::Ts,
        TargetFormat::Mpg,
        TargetFormat::ThreeGp,
        TargetFormat::Flv,
        TargetFormat::Gif,
        TargetFormat::AnimatedWebp,
        TargetFormat::Apng,
        TargetFormat::Jpg,
        TargetFormat::Png,
        TargetFormat::Webp,
        TargetFormat::Avif,
        TargetFormat::Bmp,
        TargetFormat::Tiff,
        TargetFormat::StillGif,
        TargetFormat::Ico,
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

    fn picture(codec: &str, container: &str) -> MediaInfo {
        media(&format!(
            r#"{{ "streams": [{{ "index": 0, "codec_name": "{codec}", "codec_type": "video",
            "width": 4032, "height": 3024 }}], "format": {{ "format_name": "{container}" }} }}"#
        ))
    }

    fn formats(options: &[TargetOption]) -> Vec<TargetFormat> {
        options.iter().map(|o| o.format).collect()
    }

    #[test]
    fn catalog_is_complete_and_consistent() {
        assert_eq!(FORMATS.len(), ALL.len());
        let mut ids = HashSet::new();
        for f in ALL {
            let s = spec(*f);
            assert!(ids.insert(f.id()), "duplicate id {}", f.id());
            assert_eq!(TargetFormat::from_id(&f.id()), Some(*f));
            // Formats sharing an extension must be told apart by the kind of
            // content or by the codec.
            for other in FORMATS.iter().filter(|o| o.format != *f) {
                let shared = other.extension == s.extension
                    && (other.kind == FormatKind::Image) == (s.kind == FormatKind::Image);
                assert!(
                    !shared || !s.detect_codecs.is_empty() || !other.detect_codecs.is_empty(),
                    "{f:?} and {:?} cannot be told apart",
                    other.format
                );
            }
            assert!(s.input_extensions.contains(&s.extension), "{:?}", f);
            match s.kind {
                FormatKind::Audio => {
                    assert!(
                        s.video_codecs.is_empty() && !s.audio_codecs.is_empty(),
                        "{f:?}"
                    );
                }
                FormatKind::Video => assert!(!s.video_codecs.is_empty(), "{f:?}"),
                FormatKind::Image => assert!(
                    s.video_codecs.is_empty() && s.audio_codecs.is_empty(),
                    "{f:?}"
                ),
            }
            assert_eq!(
                s.image_codec.is_some(),
                s.kind == FormatKind::Image,
                "{f:?}"
            );
        }
    }

    #[test]
    fn pictures_get_only_picture_formats() {
        let options = available_targets(&picture("mjpeg", "image2"), Path::new("IMG_1.JPEG"));
        assert!(options.iter().all(|o| o.group == TargetGroup::Image));
        assert!(options.iter().all(|o| o.kind == FormatKind::Image));
        assert!(
            options
                .iter()
                .all(|o| o.video_codecs.is_empty() && o.audio_codecs.is_empty())
        );
        assert_eq!(
            formats(&options),
            [
                TargetFormat::Jpg,
                TargetFormat::Png,
                TargetFormat::Webp,
                TargetFormat::Avif,
                TargetFormat::Bmp,
                TargetFormat::Tiff,
                TargetFormat::StillGif,
                TargetFormat::Ico,
            ]
        );
        // JPG → JPG is offered (to make the file lighter), flagged as the same format.
        assert!(options[0].same_as_input && !options[0].lossless);
        assert!(options[1].lossless);

        // PNG → PNG and BMP → BMP gain nothing; HEIC has no output format of its own.
        let png = available_targets(&picture("png", "png_pipe"), Path::new("a.png"));
        assert!(!formats(&png).contains(&TargetFormat::Png));
        let bmp = available_targets(&picture("bmp", "bmp_pipe"), Path::new("a.bmp"));
        assert!(!formats(&bmp).contains(&TargetFormat::Bmp));
        let heic = available_targets(&picture("hevc", "image2"), Path::new("a.heic"));
        assert_eq!(heic.len(), IMAGE_TARGETS.len());

        // Pictures never reach audio or video formats.
        assert!(!is_offered(
            &picture("png", "png_pipe"),
            Path::new("a.png"),
            TargetFormat::Mp4
        ));
    }

    #[test]
    fn videos_give_frames_in_the_classic_picture_formats() {
        let options = available_targets(&video_with_audio(), Path::new("clip.mp4"));
        let frames: Vec<_> = options
            .iter()
            .filter(|o| o.group == TargetGroup::Frames)
            .collect();
        assert_eq!(
            frames.iter().map(|o| o.format).collect::<Vec<_>>(),
            FRAME_TARGETS
        );
        assert!(
            frames
                .iter()
                .all(|o| o.kind == FormatKind::Image && !o.same_as_input)
        );
        // A frame of a WebP animation is a WebP picture, not "the same format".
        let anim = media(
            r#"{ "streams": [{ "index": 0, "codec_name": "webp_anim", "codec_type": "video",
            "width": 100, "height": 100 }], "format": { "format_name": "webp_anim" } }"#,
        );
        let webp = available_targets(&anim, Path::new("a.webp"));
        let still = webp
            .iter()
            .find(|o| o.format == TargetFormat::Webp)
            .unwrap();
        assert_eq!(still.group, TargetGroup::Frames);
        assert!(!still.same_as_input);
        // Frames come last.
        assert_eq!(options.last().unwrap().group, TargetGroup::Frames);
    }

    #[test]
    fn lossless_picture_detection() {
        assert!(is_lossless_image("png", Some("rgba")));
        assert!(is_lossless_image("bmp", Some("bgr24")));
        assert!(is_lossless_image("webp", Some("argb")));
        assert!(!is_lossless_image("webp", Some("yuv420p")));
        assert!(!is_lossless_image("mjpeg", Some("yuvj420p")));
        assert!(!is_lossless_image("hevc", Some("yuv420p")));
        assert!(!is_lossless_image("av1", None));
        assert_eq!(TargetFormat::from_id("JPEG"), Some(TargetFormat::Jpg));
        assert_eq!(TargetFormat::from_id("webp"), Some(TargetFormat::Webp));
    }

    #[test]
    fn every_format_is_reachable_from_some_input() {
        let offered: HashSet<TargetFormat> = RULES
            .iter()
            .flat_map(|r| r.video.iter().chain(r.audio).chain(r.image))
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
                TargetFormat::Alac,
                TargetFormat::Wav,
                TargetFormat::Ogg,
                TargetFormat::Opus,
                TargetFormat::Wma,
                TargetFormat::Aiff,
                TargetFormat::Aac,
                TargetFormat::Ac3,
                TargetFormat::Eac3,
                TargetFormat::Mp2,
                TargetFormat::Caf,
                TargetFormat::Mka,
            ]
        );
        // Less common formats are flagged for the "More formats" section.
        let secondary: Vec<_> = options
            .iter()
            .filter(|o| o.secondary)
            .map(|o| o.format)
            .collect();
        assert_eq!(
            secondary,
            [
                TargetFormat::Aac,
                TargetFormat::Ac3,
                TargetFormat::Eac3,
                TargetFormat::Mp2,
                TargetFormat::Caf,
                TargetFormat::Mka
            ]
        );
    }

    fn m4a(codec: &str) -> MediaInfo {
        media(&format!(
            r#"{{ "streams": [{{ "index": 0, "codec_name": "{codec}", "codec_type": "audio" }}],
            "format": {{ "format_name": "mov,mp4,m4a,3gp,3g2,mj2" }} }}"#
        ))
    }

    #[test]
    fn alac_and_aac_share_m4a_but_are_told_apart() {
        let aac = available_targets(&m4a("aac"), Path::new("song.m4a"));
        let find = |options: &[TargetOption], f| options.iter().find(|o| o.format == f).cloned();
        assert!(find(&aac, TargetFormat::M4a).unwrap().same_as_input);
        let alac = find(&aac, TargetFormat::Alac).unwrap();
        assert!(!alac.same_as_input && alac.lossless);
        assert_eq!(alac.extension, "m4a");
        assert_eq!(alac.audio_codecs[0].codec, AudioCodec::Alac);

        // ALAC → ALAC gains nothing; ALAC → AAC is a normal lossy conversion.
        let lossless = available_targets(&m4a("alac"), Path::new("song.m4a"));
        assert!(find(&lossless, TargetFormat::Alac).is_none());
        assert!(!find(&lossless, TargetFormat::M4a).unwrap().same_as_input);
        assert_eq!(TargetFormat::from_id("alac"), Some(TargetFormat::Alac));
        assert_eq!(TargetFormat::from_id("m4a"), Some(TargetFormat::M4a));
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
        assert!(
            options
                .iter()
                .all(|o| matches!(o.group, TargetGroup::Video | TargetGroup::Frames))
        );
        assert!(!formats(&options).contains(&TargetFormat::Gif));
        assert!(formats(&options).contains(&TargetFormat::Mp4));
        assert!(formats(&options).contains(&TargetFormat::AnimatedWebp));
    }

    #[test]
    fn animations_are_not_converted_into_themselves() {
        let anim = |codec: &str, container: &str| {
            media(&format!(
                r#"{{ "streams": [{{ "index": 0, "codec_name": "{codec}", "codec_type": "video",
                "width": 100, "height": 100 }}], "format": {{ "format_name": "{container}" }} }}"#
            ))
        };
        let webp = available_targets(&anim("webp_anim", "webp_anim"), Path::new("a.webp"));
        assert!(!formats(&webp).contains(&TargetFormat::AnimatedWebp));
        assert!(formats(&webp).contains(&TargetFormat::Gif));
        // An animated PNG named .png is an APNG, and APNG → APNG is pointless.
        let apng = available_targets(&anim("apng", "apng"), Path::new("a.png"));
        assert!(!formats(&apng).contains(&TargetFormat::Apng));
        let apng_option = available_targets(&video_with_audio(), Path::new("clip.mp4"))
            .into_iter()
            .find(|o| o.format == TargetFormat::Apng)
            .unwrap();
        assert!(apng_option.lossless && apng_option.secondary);
        assert_eq!(apng_option.extension, "apng");
        assert_eq!(TargetFormat::from_id("3gp"), Some(TargetFormat::ThreeGp));
        assert_eq!(TargetFormat::ThreeGp.id(), "3gp");
    }

    #[test]
    fn options_describe_the_codecs_for_the_advanced_panel() {
        let options = available_targets(&video_with_audio(), Path::new("clip.mp4"));
        let mkv = options
            .iter()
            .find(|o| o.format == TargetFormat::Mkv)
            .unwrap();
        assert_eq!(mkv.kind, FormatKind::Video);
        assert_eq!(
            mkv.video_codecs,
            [VideoCodec::H264, VideoCodec::Av1, VideoCodec::Vp9]
        );
        assert_eq!(mkv.audio_codecs[0].codec, AudioCodec::Aac);
        let opus = options
            .iter()
            .find(|o| o.format == TargetFormat::Opus)
            .unwrap();
        assert_eq!(
            opus.audio_codecs[0].sample_rates.as_deref(),
            Some(&[8000, 12000, 16000, 24000, 48000][..])
        );
        let gif = options
            .iter()
            .find(|o| o.format == TargetFormat::Gif)
            .unwrap();
        assert!(gif.audio_codecs.is_empty());
        let flac = options
            .iter()
            .find(|o| o.format == TargetFormat::Flac)
            .unwrap();
        assert!(flac.audio_codecs[0].lossless);
    }

    #[test]
    fn source_format_uses_the_extension_and_the_content() {
        let song = audio();
        let clip = video_with_audio();
        assert_eq!(
            source_format(&song, Path::new("a/b/Song.Flac")),
            Some(TargetFormat::Flac)
        );
        assert_eq!(
            source_format(&song, Path::new("x.aif")),
            Some(TargetFormat::Aiff)
        );
        assert_eq!(
            source_format(&clip, Path::new("x.m4v")),
            Some(TargetFormat::Mp4)
        );
        assert_eq!(source_format(&clip, Path::new("x.m2ts")), None);
        assert_eq!(source_format(&song, Path::new("noext")), None);
        // A picture named like a video format is not "the same format".
        assert_eq!(
            source_format(&picture("png", "png_pipe"), Path::new("x.mp4")),
            None
        );
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
