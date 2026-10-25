//! FFmpeg command builder.
//!
//! [`build_command`] is a pure function: analysed input + user choices + paths in,
//! argument vector out. It never touches the file system or spawns processes,
//! which keeps it fully unit-testable. The UI never sends raw FFmpeg arguments:
//! it sends a [`ConversionRequest`], [`plan_conversion`] takes the decisions and
//! [`build_from_plan`] translates them into arguments.

use std::ffi::OsString;
use std::path::Path;

use crate::error::CoreError;
use crate::formats::{self, AudioCodec, FormatSpec, VideoCodec};
use crate::plan::{
    AudioEncode, AudioPlan, AudioRate, ConversionPlan, VideoEncode, VideoPlan, VideoRate,
    plan_conversion,
};
use crate::probe::MediaInfo;
use crate::request::ConversionRequest;

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

/// Plans the conversion and builds the FFmpeg arguments to write `output`.
pub fn build_command(
    media: &MediaInfo,
    request: &ConversionRequest,
    input: &Path,
    output: &Path,
) -> Result<FfmpegCommand, CoreError> {
    let plan = plan_conversion(media, request, input)?;
    Ok(build_from_plan(&plan, input, output))
}

/// Translates a plan into FFmpeg arguments.
pub fn build_from_plan(plan: &ConversionPlan, input: &Path, output: &Path) -> FfmpegCommand {
    let spec = formats::spec(plan.target);
    let mut a = Args::default();

    // Global options: no banner, no interactive stdin, machine-readable progress.
    a.push(&[
        "-hide_banner",
        "-nostdin",
        "-nostats",
        "-loglevel",
        "warning",
    ]);
    a.push(&["-progress", "pipe:1"]);

    // Input: local files only.
    a.push(&["-protocol_whitelist", "file", "-i"]);
    a.push_os(file_url(input));

    // Stream selection: explicit maps, so subtitles/data/extra tracks are left out.
    match (&plan.video, plan.video_stream) {
        (Some(VideoPlan::Encode(enc)), Some(index)) if enc.codec == VideoCodec::Gif => {
            a.push(&[
                "-filter_complex",
                &gif_filtergraph(index, enc),
                "-map",
                "[gif]",
            ]);
        }
        (Some(_), Some(index)) => a.push(&["-map", &format!("0:{index}")]),
        _ => {}
    }
    if let (Some(_), Some(index)) = (&plan.audio, plan.audio_stream) {
        a.push(&["-map", &format!("0:{index}")]);
    }
    if let Some(index) = plan.cover_art_stream {
        a.push(&["-map", &format!("0:{index}")]);
    }
    a.push(&["-map_metadata", "0"]);

    match &plan.video {
        Some(VideoPlan::Copy { codec }) => {
            a.push(&["-c:v", "copy"]);
            // Apple players need the hvc1 tag for HEVC in MP4/MOV.
            if codec == "hevc" && matches!(spec.muxer, "mp4" | "mov") {
                a.push(&["-tag:v", "hvc1"]);
            }
        }
        Some(VideoPlan::Encode(enc)) if enc.codec != VideoCodec::Gif => video_args(&mut a, enc),
        _ => {}
    }

    match &plan.audio {
        Some(AudioPlan::Copy { .. }) => a.push(&["-c:a", "copy"]),
        Some(AudioPlan::Encode(enc)) => audio_args(&mut a, enc, spec),
        None => {}
    }

    // Cover art is only planned for audio outputs, so `v:0` is the picture.
    if plan.cover_art_stream.is_some() {
        a.push(&["-c:v", "copy", "-disposition:v:0", "attached_pic"]);
    }

    a.push(spec.muxer_args);
    a.push(&["-f", spec.muxer, "-y"]);
    a.push_os(file_url(output));
    FfmpegCommand { args: a.0 }
}

fn video_args(a: &mut Args, enc: &VideoEncode) {
    let bitrate = |a: &mut Args, kbps: u32| {
        a.push(&[
            "-b:v",
            &format!("{kbps}k"),
            "-maxrate",
            &format!("{}k", kbps * 3 / 2),
            "-bufsize",
            &format!("{}k", kbps * 2),
        ]);
    };
    match enc.codec {
        VideoCodec::H264 => {
            a.push(&["-c:v", "libopenh264", "-profile:v", "high"]);
            if let VideoRate::Bitrate { kbps } = enc.rate {
                bitrate(a, kbps);
            }
        }
        VideoCodec::Vp9 => {
            a.push(&["-c:v", "libvpx-vp9", "-row-mt", "1", "-deadline", "good"]);
            match enc.rate {
                VideoRate::Crf { crf, speed } => a.push(&[
                    "-crf",
                    &crf.to_string(),
                    "-b:v",
                    "0",
                    "-cpu-used",
                    &speed.to_string(),
                ]),
                VideoRate::Bitrate { kbps } => {
                    bitrate(a, kbps);
                    a.push(&["-cpu-used", "3"]);
                }
                _ => {}
            }
        }
        VideoCodec::Av1 => {
            a.push(&["-c:v", "libsvtav1"]);
            match enc.rate {
                VideoRate::Crf { crf, speed } => {
                    a.push(&["-crf", &crf.to_string(), "-preset", &speed.to_string()]);
                }
                VideoRate::Bitrate { kbps } => {
                    a.push(&["-b:v", &format!("{kbps}k"), "-preset", "8"]);
                }
                _ => {}
            }
        }
        VideoCodec::Mpeg4 => {
            // The "xvid" tag makes the AVI playable by more players.
            a.push(&["-c:v", "mpeg4", "-vtag", "xvid"]);
            match enc.rate {
                VideoRate::Quantizer { q } => a.push(&["-q:v", &q.to_string()]),
                VideoRate::Bitrate { kbps } => bitrate(a, kbps),
                _ => {}
            }
        }
        VideoCodec::Gif => unreachable!("GIF uses a filter graph"),
    }

    let mut filters = Vec::new();
    if let (true, Some(w), Some(h)) = (enc.resize, enc.width, enc.height) {
        filters.push(format!("scale={w}:{h}:flags=bicubic"));
    }
    if let Some(fps) = enc.fps {
        filters.push(format!("fps={}", format_number(fps)));
    }
    if !filters.is_empty() {
        a.push(&["-vf", &filters.join(",")]);
    }
    // 8-bit 4:2:0: what every player and OpenH264 support.
    a.push(&["-pix_fmt", "yuv420p"]);
}

/// Palette-optimized GIF in a single pass of the filter graph.
fn gif_filtergraph(index: u32, enc: &VideoEncode) -> String {
    let fps = format_number(enc.fps.unwrap_or(10.0));
    let scale = match (enc.width, enc.height) {
        (Some(w), Some(h)) => format!("{w}:{h}"),
        _ => "480:-1".to_owned(),
    };
    format!(
        "[0:{index}]fps={fps},scale={scale}:flags=lanczos,split[s0][s1];\
         [s0]palettegen=stats_mode=diff[p];\
         [s1][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle[gif]"
    )
}

fn audio_args(a: &mut Args, enc: &AudioEncode, spec: &FormatSpec) {
    let pcm_encoder = |bits: u32| {
        let endian = if spec.muxer == "aiff" { "be" } else { "le" };
        format!("pcm_s{bits}{endian}")
    };
    match (enc.codec, enc.rate) {
        (AudioCodec::Mp3, AudioRate::Vbr { quality }) => {
            a.push(&["-c:a", "libmp3lame", "-q:a", &quality.to_string()]);
        }
        (AudioCodec::Vorbis, AudioRate::Vbr { quality }) => {
            a.push(&["-c:a", "libvorbis", "-q:a", &quality.to_string()]);
        }
        (codec, AudioRate::Bitrate { kbps }) => {
            let encoder = match codec {
                AudioCodec::Mp3 => "libmp3lame",
                AudioCodec::Aac => "aac",
                AudioCodec::Vorbis => "libvorbis",
                AudioCodec::Opus => "libopus",
                AudioCodec::Wma => "wmav2",
                AudioCodec::Pcm | AudioCodec::Flac => {
                    unreachable!("lossless codecs have no bit rate")
                }
            };
            a.push(&["-c:a", encoder, "-b:a", &format!("{kbps}k")]);
            if codec == AudioCodec::Opus {
                a.push(&["-vbr", "on"]);
            }
        }
        (_, AudioRate::Pcm { bits }) => a.push(&["-c:a", &pcm_encoder(bits)]),
        (_, AudioRate::Flac { level, bits }) => {
            a.push(&["-c:a", "flac", "-compression_level", &level.to_string()]);
            if bits > 16 {
                a.push(&[
                    "-sample_fmt",
                    "s32",
                    "-bits_per_raw_sample",
                    &bits.to_string(),
                ]);
            } else {
                a.push(&["-sample_fmt", "s16"]);
            }
        }
        (codec, rate) => unreachable!("invalid plan: {codec:?} with {rate:?}"),
    }
    if let Some(rate) = enc.sample_rate {
        a.push(&["-ar", &rate.to_string()]);
    }
    if let Some(channels) = enc.channels {
        a.push(&["-ac", &channels.to_string()]);
    }
}

/// `25` instead of `25.0`, `29.97` kept as is.
fn format_number(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[derive(Default)]
struct Args(Vec<OsString>);

impl Args {
    fn push(&mut self, items: &[&str]) {
        self.0.extend(items.iter().map(OsString::from));
    }

    fn push_os(&mut self, item: OsString) {
        self.0.push(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::TargetFormat;
    use crate::probe::parse_ffprobe_json;
    use crate::request::{AdvancedOptions, ChannelLayout, QualityPreset};

    const HEAD: &[&str] = &[
        "-hide_banner",
        "-nostdin",
        "-nostats",
        "-loglevel",
        "warning",
        "-progress",
        "pipe:1",
        "-protocol_whitelist",
        "file",
        "-i",
    ];

    fn media(json: &str) -> MediaInfo {
        parse_ffprobe_json(json.as_bytes()).unwrap()
    }

    fn wav() -> MediaInfo {
        media(
            r#"{ "streams": [{ "index": 0, "codec_name": "pcm_s16le", "codec_type": "audio",
            "sample_rate": "44100", "channels": 2, "bits_per_sample": 16 }],
            "format": { "format_name": "wav", "duration": "3.0" } }"#,
        )
    }

    fn flac_with_cover() -> MediaInfo {
        media(
            r#"{ "streams": [
            { "index": 0, "codec_name": "flac", "codec_type": "audio", "sample_rate": "96000",
              "channels": 2, "bits_per_raw_sample": "24" },
            { "index": 1, "codec_name": "png", "codec_type": "video", "disposition": { "attached_pic": 1 } }],
            "format": { "format_name": "flac", "duration": "100" } }"#,
        )
    }

    fn mp4() -> MediaInfo {
        media(
            r#"{ "streams": [
            { "index": 0, "codec_name": "h264", "codec_type": "video", "width": 1920, "height": 1080,
              "avg_frame_rate": "30/1", "bit_rate": "6000000" },
            { "index": 1, "codec_name": "aac", "codec_type": "audio", "sample_rate": "48000",
              "channels": 2, "bit_rate": "192000" }],
            "format": { "format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "60" } }"#,
        )
    }

    /// Arguments between the input URL and `-f` (stream selection and codecs).
    fn body(media: &MediaInfo, request: &ConversionRequest, input: &str) -> Vec<String> {
        let cmd = build_command(media, request, Path::new(input), Path::new("out.tmp")).unwrap();
        let args = cmd.args_lossy();
        assert_eq!(&args[..HEAD.len()], HEAD);
        assert_eq!(args[HEAD.len()], format!("file:{input}"));
        let f = args.iter().rposition(|a| a == "-f").unwrap();
        assert_eq!(&args[f + 2..], ["-y", "file:out.tmp"]);
        args[HEAD.len() + 1..f].to_vec()
    }

    fn req(target: TargetFormat, preset: QualityPreset) -> ConversionRequest {
        ConversionRequest::new(target, preset)
    }

    #[test]
    #[rustfmt::skip]
    fn wav_to_mp3_balanced_full_command() {
        let cmd = build_command(&wav(), &req(TargetFormat::Mp3, QualityPreset::Balanced),
            Path::new("in put.wav"), Path::new("out.mp3")).unwrap();
        assert_eq!(
            cmd.args_lossy(),
            [
                "-hide_banner", "-nostdin", "-nostats", "-loglevel", "warning",
                "-progress", "pipe:1",
                "-protocol_whitelist", "file", "-i", "file:in put.wav",
                "-map", "0:0", "-map_metadata", "0",
                "-c:a", "libmp3lame", "-q:a", "2",
                "-id3v2_version", "3",
                "-f", "mp3", "-y", "file:out.mp3",
            ]
        );
    }

    #[test]
    #[rustfmt::skip]
    fn every_audio_target_from_wav() {
        let cases: &[(TargetFormat, &[&str])] = &[
            (TargetFormat::M4a, &["-map", "0:0", "-map_metadata", "0", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"]),
            (TargetFormat::Flac, &["-map", "0:0", "-map_metadata", "0", "-c:a", "flac", "-compression_level", "5", "-sample_fmt", "s16"]),
            (TargetFormat::Ogg, &["-map", "0:0", "-map_metadata", "0", "-c:a", "libvorbis", "-q:a", "6"]),
            (TargetFormat::Opus, &["-map", "0:0", "-map_metadata", "0", "-c:a", "libopus", "-b:a", "128k", "-vbr", "on", "-ar", "48000"]),
            (TargetFormat::Wma, &["-map", "0:0", "-map_metadata", "0", "-c:a", "wmav2", "-b:a", "160k"]),
            (TargetFormat::Aiff, &["-map", "0:0", "-map_metadata", "0", "-c:a", "pcm_s16be"]),
        ];
        for (target, expected) in cases {
            assert_eq!(body(&wav(), &req(*target, QualityPreset::Balanced), "a.wav"), *expected, "{target:?}");
        }
    }

    #[test]
    fn muxers_match_the_catalog() {
        for (target, muxer) in [
            (TargetFormat::M4a, "ipod"),
            (TargetFormat::Wma, "asf"),
            (TargetFormat::Opus, "opus"),
        ] {
            let cmd = build_command(
                &wav(),
                &req(target, QualityPreset::Balanced),
                Path::new("a.wav"),
                Path::new("o"),
            )
            .unwrap()
            .args_lossy();
            let f = cmd.iter().position(|a| a == "-f").unwrap();
            assert_eq!(cmd[f + 1], muxer);
        }
    }

    #[test]
    #[rustfmt::skip]
    fn high_resolution_flac_to_wav_and_m4a_with_cover() {
        let src = flac_with_cover();
        assert_eq!(
            body(&src, &req(TargetFormat::Wav, QualityPreset::Max), "a.flac"),
            ["-map", "0:0", "-map_metadata", "0", "-c:a", "pcm_s24le", "-rf64", "auto"]
        );
        assert_eq!(
            body(&src, &req(TargetFormat::M4a, QualityPreset::Small), "a.flac"),
            ["-map", "0:0", "-map", "0:1", "-map_metadata", "0", "-c:a", "aac", "-b:a", "128k",
             "-c:v", "copy", "-disposition:v:0", "attached_pic", "-movflags", "+faststart"]
        );
        assert_eq!(
            body(&src, &req(TargetFormat::Mp3, QualityPreset::Max), "a.flac"),
            ["-map", "0:0", "-map", "0:1", "-map_metadata", "0", "-c:a", "libmp3lame", "-q:a", "0",
             "-ar", "48000", "-c:v", "copy", "-disposition:v:0", "attached_pic", "-id3v2_version", "3"]
        );
    }

    #[test]
    #[rustfmt::skip]
    fn video_targets_from_mp4() {
        let src = mp4();
        // Same codecs fit MKV: plain remux.
        assert_eq!(
            body(&src, &req(TargetFormat::Mkv, QualityPreset::Balanced), "clip.mp4"),
            ["-map", "0:0", "-map", "0:1", "-map_metadata", "0", "-c:v", "copy", "-c:a", "copy"]
        );
        assert_eq!(
            body(&src, &req(TargetFormat::Mp4, QualityPreset::Balanced), "clip.mp4"),
            ["-map", "0:0", "-map", "0:1", "-map_metadata", "0",
             "-c:v", "libopenh264", "-profile:v", "high", "-b:v", "4350k", "-maxrate", "6525k", "-bufsize", "8700k",
             "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"]
        );
        assert_eq!(
            body(&src, &req(TargetFormat::Webm, QualityPreset::Max), "clip.mp4"),
            ["-map", "0:0", "-map", "0:1", "-map_metadata", "0",
             "-c:v", "libvpx-vp9", "-row-mt", "1", "-deadline", "good", "-crf", "24", "-b:v", "0", "-cpu-used", "2",
             "-pix_fmt", "yuv420p", "-c:a", "libopus", "-b:a", "192k", "-vbr", "on"]
        );
        assert_eq!(
            body(&src, &req(TargetFormat::Avi, QualityPreset::Small), "clip.mp4"),
            ["-map", "0:0", "-map", "0:1", "-map_metadata", "0",
             "-c:v", "mpeg4", "-vtag", "xvid", "-q:v", "7", "-pix_fmt", "yuv420p",
             "-c:a", "libmp3lame", "-b:a", "128k"]
        );
        assert_eq!(
            body(&src, &req(TargetFormat::Mov, QualityPreset::Max), "clip.mp4"),
            ["-map", "0:0", "-map", "0:1", "-map_metadata", "0", "-c:v", "copy", "-c:a", "copy",
             "-movflags", "+faststart"]
        );
    }

    #[test]
    #[rustfmt::skip]
    fn gif_uses_a_palette_filtergraph_without_audio() {
        assert_eq!(
            body(&mp4(), &req(TargetFormat::Gif, QualityPreset::Small), "clip.mp4"),
            ["-filter_complex",
             "[0:0]fps=10,scale=480:270:flags=lanczos,split[s0][s1];[s0]palettegen=stats_mode=diff[p];\
              [s1][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle[gif]",
             "-map", "[gif]", "-map_metadata", "0", "-loop", "0"]
        );
    }

    #[test]
    #[rustfmt::skip]
    fn audio_extraction_maps_only_the_audio_stream() {
        assert_eq!(
            body(&mp4(), &req(TargetFormat::M4a, QualityPreset::Balanced), "clip.mp4"),
            ["-map", "0:1", "-map_metadata", "0", "-c:a", "copy", "-movflags", "+faststart"]
        );
        assert_eq!(
            body(&mp4(), &req(TargetFormat::Flac, QualityPreset::Balanced), "clip.mp4"),
            ["-map", "0:1", "-map_metadata", "0", "-c:a", "flac", "-compression_level", "5", "-sample_fmt", "s16"]
        );
    }

    #[test]
    #[rustfmt::skip]
    fn advanced_options_become_filters_and_codec_flags() {
        let mut request = req(TargetFormat::Mkv, QualityPreset::Balanced);
        request.advanced = AdvancedOptions {
            video_codec: Some(VideoCodec::Av1),
            max_height: Some(720),
            fps: Some(29.97),
            audio_codec: Some(AudioCodec::Opus),
            channels: Some(ChannelLayout::Mono),
            audio_bitrate_kbps: Some(96),
            ..Default::default()
        };
        assert_eq!(
            body(&mp4(), &request, "clip.mp4"),
            ["-map", "0:0", "-map", "0:1", "-map_metadata", "0",
             "-c:v", "libsvtav1", "-crf", "34", "-preset", "8",
             "-vf", "scale=1280:720:flags=bicubic,fps=29.97", "-pix_fmt", "yuv420p",
             "-c:a", "libopus", "-b:a", "96k", "-vbr", "on", "-ac", "1"]
        );
    }

    #[test]
    fn hevc_copy_into_mp4_gets_the_apple_tag() {
        let src = media(
            r#"{ "streams": [{ "index": 0, "codec_name": "hevc", "codec_type": "video",
            "width": 3840, "height": 2160 }], "format": { "format_name": "matroska,webm" } }"#,
        );
        let args = body(
            &src,
            &req(TargetFormat::Mp4, QualityPreset::Max),
            "clip.mkv",
        );
        assert!(args.windows(2).any(|w| w == ["-tag:v", "hvc1"]));
    }

    #[test]
    fn file_names_starting_with_a_dash_stay_file_urls() {
        let cmd = build_command(
            &wav(),
            &req(TargetFormat::Mp3, QualityPreset::Small),
            Path::new("-y.wav"),
            Path::new("http:x.mp3"),
        )
        .unwrap();
        let args = cmd.args_lossy();
        assert!(args.contains(&"file:-y.wav".to_string()));
        assert_eq!(args.last().unwrap(), "file:http:x.mp3");
    }

    #[test]
    fn format_number_trims_zeros() {
        assert_eq!(format_number(25.0), "25");
        assert_eq!(format_number(29.97), "29.97");
        assert_eq!(format_number(23.976_023), "23.976");
    }
}
