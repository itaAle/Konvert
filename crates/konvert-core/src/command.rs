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
use crate::formats::{self, AudioCodec, FormatSpec, ImageCodec, VideoCodec};
use crate::plan::{
    AudioEncode, AudioPlan, AudioRate, ConversionPlan, FramePlan, ImagePlan, ImageQuality,
    VideoEncode, VideoPlan, VideoRate, plan_conversion,
};
use crate::probe::MediaInfo;
use crate::request::{ConversionRequest, FrameMode};

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
    build_from_plan_with(plan, input, None, output)
}

/// Like [`build_from_plan`], with an `ffmetadata` file holding the cover
/// picture comment of an Ogg output (see [`crate::metadata`]).
pub fn build_from_plan_with(
    plan: &ConversionPlan,
    input: &Path,
    cover_comment: Option<&Path>,
    output: &Path,
) -> FfmpegCommand {
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
    a.push(&["-protocol_whitelist", "file"]);
    if let Some(at) = plan.frames.and_then(|f| f.at_secs) {
        // Seeking before `-i` decodes from the previous key frame: fast and exact.
        a.push(&["-ss", &format_number(at)]);
    }
    a.push(&["-i"]);
    a.push_os(file_url(input));
    if let Some(comment) = cover_comment {
        a.push(&["-protocol_whitelist", "file", "-f", "ffmetadata", "-i"]);
        a.push_os(file_url(comment));
    }

    match &plan.image {
        Some(image) => picture_args(
            &mut a,
            image,
            plan.video_stream.unwrap_or(0),
            plan.frames.as_ref(),
        ),
        None => media_args(&mut a, plan, spec, cover_comment.is_some()),
    }

    // A series of pictures: one file per picture, named from a pattern.
    let sequence = plan.frames.is_some_and(|f| f.mode == FrameMode::Sequence);
    let muxer = if sequence { "image2" } else { spec.muxer };
    a.push(spec.muxer_args);
    a.push(&["-f", muxer, "-y"]);
    a.push_os(file_url(output));
    FfmpegCommand { args: a.0 }
}

/// Stream selection and codecs of an audio or video conversion.
fn media_args(a: &mut Args, plan: &ConversionPlan, spec: &FormatSpec, cover_comment: bool) {
    // The cover travels as an attached picture, or as a comment (Ogg).
    let attached_cover = plan.cover_art_stream.filter(|_| spec.cover_art);
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
    if let Some(index) = attached_cover {
        a.push(&["-map", &format!("0:{index}")]);
    }
    a.push(&["-map_metadata", "0"]);
    if cover_comment {
        // The second input's global section holds the picture comment.
        a.push(&["-map_metadata:s:a:0", "1:g"]);
    }
    if plan.remove_location {
        // An empty value removes the tag.
        for key in [
            "location",
            "location-eng",
            "com.apple.quicktime.location.ISO6709",
        ] {
            a.push(&["-metadata", &format!("{key}=")]);
        }
    }

    match &plan.video {
        Some(VideoPlan::Copy { codec }) => {
            a.push(&["-c:v", "copy"]);
            // Apple players need the hvc1 tag for HEVC in MP4/MOV.
            if codec == "hevc" && matches!(spec.muxer, "mp4" | "mov") {
                a.push(&["-tag:v", "hvc1"]);
            }
        }
        Some(VideoPlan::Encode(enc)) if enc.codec != VideoCodec::Gif => video_args(a, enc),
        _ => {}
    }

    match &plan.audio {
        Some(AudioPlan::Copy { .. }) => a.push(&["-c:a", "copy"]),
        Some(AudioPlan::Encode(enc)) => audio_args(a, enc, spec),
        None => {}
    }

    // Cover art is only planned for audio outputs, so `v:0` is the picture.
    if attached_cover.is_some() {
        a.push(&["-c:v", "copy", "-disposition:v:0", "attached_pic"]);
    }
}

/// One picture. The filter graph assembles the tiles of a grid (or merges a
/// separate transparency plane), scales, lays transparent areas on white when
/// the format has no transparency and prepares the encoder's pixel format.
/// FFmpeg applies the EXIF/HEIF rotation before the graph.
fn picture_args(a: &mut Args, image: &ImagePlan, index: u32, frames: Option<&FramePlan>) {
    let sequence = frames.is_some_and(|f| f.mode == FrameMode::Sequence);
    let (inputs, mut steps) = match (image.grid_group, image.alpha_stream) {
        (Some(group), _) => (format!("[0:g:{group}]"), Vec::new()),
        (None, Some(alpha)) => (
            format!("[0:{index}][0:{alpha}]"),
            vec!["alphamerge".to_owned()],
        ),
        (None, None) => (format!("[0:{index}]"), Vec::new()),
    };
    if !image.icons.is_empty() {
        return icon_args(a, image, &inputs, steps);
    }
    if let Some(every) = frames.and_then(|f| f.every_secs) {
        steps.push(format!("fps=1/{}", format_number(every)));
    }
    if let (true, Some(w), Some(h)) = (image.resize, image.width, image.height) {
        steps.push(format!("scale={w}:{h}:flags=lanczos"));
    }
    if image.flatten {
        steps.push(
            "format=rgba,split[fg][back];\
             [back]drawbox=c=white@1:t=fill:replace=1,format=rgb24[bg];\
             [bg][fg]overlay=format=auto"
                .to_owned(),
        );
    }
    // AVIF stores transparency as a second, grayscale picture.
    let alpha_plane = image.codec == ImageCodec::Avif && image.alpha;
    if alpha_plane {
        steps.push(
            "format=yuva420p,split[color][a];[a]alphaextract[alpha];[color]format=yuv420p"
                .to_owned(),
        );
    } else if image.codec == ImageCodec::Avif {
        steps.push("format=yuv420p".to_owned());
    }
    if image.codec == ImageCodec::Gif {
        // One palette for this picture; a transparent entry only if needed.
        let (reserve, threshold) = if image.alpha {
            (1, ":alpha_threshold=128")
        } else {
            (0, "")
        };
        steps.push(format!(
            "split[g0][g1];[g0]palettegen=reserve_transparent={reserve}:stats_mode=single[p];\
             [g1][p]paletteuse{threshold}"
        ));
    }
    if steps.is_empty() {
        steps.push("null".to_owned());
    }
    a.push(&[
        "-filter_complex",
        &format!("{inputs}{}[img]", steps.join(",")),
        "-map",
        "[img]",
    ]);
    if alpha_plane {
        a.push(&["-map", "[alpha]"]);
    }
    if sequence {
        // One file per decoded (or sampled) frame, no duplicates.
        a.push(&["-fps_mode", "passthrough"]);
    } else {
        a.push(&["-frames:v", "1"]);
    }

    match (image.codec, image.quality) {
        (ImageCodec::Jpeg, ImageQuality::Lossy { quality }) => {
            // Full chroma resolution at high quality; full-range YUV as JPEG expects.
            let pix_fmt = if quality >= 90 { "yuv444p" } else { "yuv420p" };
            a.push(&[
                "-c:v",
                "mjpeg",
                "-q:v",
                &jpeg_qscale(quality).to_string(),
                "-pix_fmt",
                pix_fmt,
                "-color_range",
                "pc",
            ]);
        }
        (ImageCodec::Png, ImageQuality::Lossless { effort }) => {
            a.push(&["-c:v", "png", "-compression_level", &effort.to_string()]);
        }
        (ImageCodec::Webp, ImageQuality::Lossless { effort }) => {
            a.push(&[
                "-c:v",
                "libwebp",
                "-lossless",
                "1",
                "-compression_level",
                &effort.to_string(),
            ]);
        }
        (ImageCodec::Webp, ImageQuality::Lossy { quality }) => {
            let pix_fmt = if image.alpha { "yuva420p" } else { "yuv420p" };
            a.push(&[
                "-c:v",
                "libwebp",
                "-quality",
                &quality.to_string(),
                "-pix_fmt",
                pix_fmt,
            ]);
        }
        (ImageCodec::Avif, ImageQuality::Lossy { quality }) => {
            a.push(&[
                "-c:v",
                "libaom-av1",
                "-still-picture",
                "1",
                "-cpu-used",
                "6",
                "-crf",
                &avif_crf(quality).to_string(),
                "-b:v",
                "0",
            ]);
        }
        (ImageCodec::Bmp, _) => a.push(&["-c:v", "bmp", "-pix_fmt", "bgr24"]),
        (ImageCodec::Tiff, _) => {
            let pix_fmt = if image.alpha { "rgba" } else { "rgb24" };
            a.push(&[
                "-c:v",
                "tiff",
                "-compression_algo",
                "deflate",
                "-pix_fmt",
                pix_fmt,
            ]);
        }
        (ImageCodec::Gif, _) => a.push(&["-c:v", "gif"]),
        (ImageCodec::Jxl, quality) => {
            let distance = match quality {
                ImageQuality::Lossy { quality } => jxl_distance(quality),
                _ => 0.0,
            };
            let effort = match quality {
                ImageQuality::Lossless { effort } => effort,
                _ => 7,
            };
            let pix_fmt = if image.alpha { "rgba" } else { "rgb24" };
            a.push(&[
                "-c:v",
                "libjxl",
                "-distance",
                &format_number(distance),
                "-effort",
                &effort.to_string(),
                "-pix_fmt",
                pix_fmt,
            ]);
        }
        (codec, quality) => unreachable!("invalid plan: {codec:?} with {quality:?}"),
    }
}

/// Windows icon: one PNG picture per size, each fitted into a transparent
/// square, all in one `.ico` file.
fn icon_args(a: &mut Args, image: &ImagePlan, inputs: &str, mut steps: Vec<String>) {
    let count = image.icons.len();
    steps.push("format=rgba".to_owned());
    let branches: String = (0..count).map(|i| format!("[i{i}]")).collect();
    let mut graph = format!("{inputs}{},split={count}{branches}", steps.join(","));
    for (i, icon) in image.icons.iter().enumerate() {
        let (x, y) = ((icon.size - icon.width) / 2, (icon.size - icon.height) / 2);
        graph.push_str(&format!(
            ";[i{i}]scale={}:{}:flags=lanczos,pad={s}:{s}:{x}:{y}:color=black@0[s{i}]",
            icon.width,
            icon.height,
            s = icon.size
        ));
    }
    a.push(&["-filter_complex", &graph]);
    for i in 0..count {
        a.push(&["-map", &format!("[s{i}]")]);
    }
    a.push(&["-frames:v", "1", "-c:v", "png", "-compression_level", "9"]);
}

/// FFmpeg's JPEG quantizer scale (2 = best … 31) for a quality from 1 to 100.
fn jpeg_qscale(quality: u8) -> u8 {
    let steps = (100 - quality.min(100)) as f64 / 5.0;
    (steps.round() as u8 + 1).clamp(2, 31)
}

/// JPEG XL distance (0 = lossless, 1 = visually lossless … 25) for a quality
/// from 1 to 100, with libjxl's own mapping.
fn jxl_distance(quality: u8) -> f64 {
    let q = quality.min(100) as f64;
    // 100 means mathematically lossless.
    let distance = if q >= 100.0 {
        0.0
    } else if q >= 30.0 {
        0.1 + (100.0 - q) * 0.09
    } else {
        53.0 / 3000.0 * q * q - 23.0 / 20.0 * q + 25.0
    };
    (distance * 100.0).round() / 100.0
}

/// AV1 constant quality (0 = best … 63) for a quality from 1 to 100, with the
/// same scale as libavif.
fn avif_crf(quality: u8) -> u8 {
    ((100 - quality.min(100) as u32) * 63 + 50).div_euclid(100) as u8
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
        VideoCodec::Mpeg4 | VideoCodec::Mpeg2 => {
            if enc.codec == VideoCodec::Mpeg4 {
                // The "xvid" tag makes the AVI playable by more players.
                a.push(&["-c:v", "mpeg4", "-vtag", "xvid"]);
            } else {
                a.push(&["-c:v", "mpeg2video", "-bf", "2"]);
            }
            match enc.rate {
                VideoRate::Quantizer { q } => a.push(&["-q:v", &q.to_string()]),
                VideoRate::Bitrate { kbps } => bitrate(a, kbps),
                _ => {}
            }
        }
        VideoCodec::Webp => {
            a.push(&["-c:v", "libwebp_anim"]);
            if let VideoRate::Quality { quality } = enc.rate {
                a.push(&["-quality", &quality.to_string()]);
            }
        }
        VideoCodec::Apng => a.push(&["-c:v", "apng", "-pred", "mixed"]),
        VideoCodec::Gif => unreachable!("GIF uses a filter graph"),
    }

    let mut filters = Vec::new();
    if let (true, Some(w), Some(h)) = (enc.resize, enc.width, enc.height) {
        // Animations are small: the sharper (slower) filter is affordable.
        let flags = if enc.codec.is_animation() {
            "lanczos"
        } else {
            "bicubic"
        };
        filters.push(format!("scale={w}:{h}:flags={flags}"));
    }
    if let Some(fps) = enc.fps {
        filters.push(format!("fps={}", format_number(fps)));
    }
    if !filters.is_empty() {
        a.push(&["-vf", &filters.join(",")]);
    }
    let pix_fmt = match (enc.codec, enc.alpha) {
        (VideoCodec::Webp, true) => "yuva420p",
        (VideoCodec::Apng, true) => "rgba",
        (VideoCodec::Apng, false) => "rgb24",
        // 8-bit 4:2:0: what every player and OpenH264 support.
        _ => "yuv420p",
    };
    a.push(&["-pix_fmt", pix_fmt]);
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
                AudioCodec::Ac3 => "ac3",
                AudioCodec::Eac3 => "eac3",
                AudioCodec::Mp2 => "mp2",
                AudioCodec::Pcm | AudioCodec::Flac | AudioCodec::Alac => {
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
        (_, AudioRate::Alac { bits }) => {
            a.push(&["-c:a", "alac"]);
            if bits > 16 {
                a.push(&[
                    "-sample_fmt",
                    "s32p",
                    "-bits_per_raw_sample",
                    &bits.to_string(),
                ]);
            } else {
                a.push(&["-sample_fmt", "s16p"]);
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
            (TargetFormat::Aiff, &["-map", "0:0", "-map_metadata", "0", "-c:a", "pcm_s16be", "-write_id3v2", "1"]),
            (TargetFormat::Alac, &["-map", "0:0", "-map_metadata", "0", "-c:a", "alac", "-sample_fmt", "s16p", "-movflags", "+faststart"]),
            (TargetFormat::Aac, &["-map", "0:0", "-map_metadata", "0", "-c:a", "aac", "-b:a", "192k", "-write_id3v2", "1"]),
            (TargetFormat::Ac3, &["-map", "0:0", "-map_metadata", "0", "-c:a", "ac3", "-b:a", "384k"]),
            (TargetFormat::Eac3, &["-map", "0:0", "-map_metadata", "0", "-c:a", "eac3", "-b:a", "256k"]),
            (TargetFormat::Mp2, &["-map", "0:0", "-map_metadata", "0", "-c:a", "mp2", "-b:a", "256k"]),
            (TargetFormat::Caf, &["-map", "0:0", "-map_metadata", "0", "-c:a", "alac", "-sample_fmt", "s16p"]),
            (TargetFormat::Mka, &["-map", "0:0", "-map_metadata", "0", "-c:a", "copy"]),
        ];
        for (target, expected) in cases {
            assert_eq!(body(&wav(), &req(*target, QualityPreset::Balanced), "a.wav"), *expected, "{target:?}");
        }
    }

    #[test]
    fn muxers_match_the_catalog() {
        for (target, muxer) in [
            (TargetFormat::M4a, "ipod"),
            (TargetFormat::Alac, "ipod"),
            (TargetFormat::Wma, "asf"),
            (TargetFormat::Opus, "opus"),
            (TargetFormat::Aac, "adts"),
            (TargetFormat::Caf, "caf"),
            (TargetFormat::Mka, "matroska"),
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
        assert_eq!(
            body(&src, &req(TargetFormat::Alac, QualityPreset::Max), "a.flac"),
            ["-map", "0:0", "-map", "0:1", "-map_metadata", "0",
             "-c:a", "alac", "-sample_fmt", "s32p", "-bits_per_raw_sample", "24",
             "-c:v", "copy", "-disposition:v:0", "attached_pic", "-movflags", "+faststart"]
        );
    }

    #[test]
    fn location_tags_are_emptied_on_request() {
        let mut src = mp4();
        src.has_location = true;
        let mut request = req(TargetFormat::Mkv, QualityPreset::Balanced);
        let args = body(&src, &request, "clip.mov");
        assert!(!args.iter().any(|a| a.starts_with("location")));
        request.remove_location = true;
        let args = body(&src, &request, "clip.mov");
        for key in [
            "location=",
            "location-eng=",
            "com.apple.quicktime.location.ISO6709=",
        ] {
            assert!(args.windows(2).any(|w| w == ["-metadata", key]), "{key}");
        }
        // Nothing to remove: no extra arguments.
        src.has_location = false;
        assert!(!body(&src, &request, "clip.mov").contains(&"-metadata".to_owned()));
    }

    #[test]
    #[rustfmt::skip]
    fn ogg_covers_travel_as_a_comment_file() {
        let src = flac_with_cover();
        let plan = plan_conversion(&src, &req(TargetFormat::Opus, QualityPreset::Balanced), Path::new("a.flac")).unwrap();
        let cmd = build_from_plan_with(&plan, Path::new("a.flac"), Some(Path::new("cover.txt")), Path::new("o.opus"))
            .args_lossy();
        let i = cmd.iter().position(|a| a == "file:a.flac").unwrap();
        assert_eq!(cmd[i + 1..i + 7], ["-protocol_whitelist", "file", "-f", "ffmetadata", "-i", "file:cover.txt"]);
        // The picture itself is not mapped: Ogg cannot hold it as a stream.
        assert!(!cmd.contains(&"0:1".to_owned()));
        assert!(cmd.windows(2).any(|w| w == ["-map_metadata:s:a:0", "1:g"]));
        assert!(!cmd.contains(&"attached_pic".to_owned()));
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
    fn animated_webp_apng_and_mpeg2() {
        assert_eq!(
            body(&mp4(), &req(TargetFormat::AnimatedWebp, QualityPreset::Balanced), "clip.mp4"),
            ["-map", "0:0", "-map_metadata", "0", "-c:v", "libwebp_anim", "-quality", "75",
             "-vf", "scale=640:360:flags=lanczos,fps=15", "-pix_fmt", "yuv420p", "-loop", "0"]
        );
        assert_eq!(
            body(&mp4(), &req(TargetFormat::Apng, QualityPreset::Small), "clip.mp4"),
            ["-map", "0:0", "-map_metadata", "0", "-c:v", "apng", "-pred", "mixed",
             "-vf", "scale=480:270:flags=lanczos,fps=10", "-pix_fmt", "rgb24", "-plays", "0"]
        );
        assert_eq!(
            body(&mp4(), &req(TargetFormat::Mpg, QualityPreset::Max), "clip.mp4"),
            ["-map", "0:0", "-map", "0:1", "-map_metadata", "0",
             "-c:v", "mpeg2video", "-bf", "2", "-q:v", "2", "-pix_fmt", "yuv420p",
             "-c:a", "mp2", "-b:a", "384k"]
        );
        let cmd = build_command(&mp4(), &req(TargetFormat::Mpg, QualityPreset::Max),
            Path::new("a.mp4"), Path::new("o")).unwrap().args_lossy();
        assert!(cmd.windows(2).any(|w| w == ["-f", "vob"]));
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

    /// Arguments between the input URL and `-f`, plus the muxer.
    fn picture_body(
        media: &MediaInfo,
        request: &ConversionRequest,
        input: &str,
    ) -> (Vec<String>, String) {
        let cmd = build_command(media, request, Path::new(input), Path::new("out.tmp")).unwrap();
        let args = cmd.args_lossy();
        assert_eq!(&args[..HEAD.len()], HEAD);
        let f = args.iter().rposition(|a| a == "-f").unwrap();
        assert_eq!(&args[f + 2..], ["-y", "file:out.tmp"]);
        (args[HEAD.len() + 1..f].to_vec(), args[f + 1].clone())
    }

    fn jpeg() -> MediaInfo {
        media(
            r#"{ "streams": [{ "index": 0, "codec_name": "mjpeg", "codec_type": "video",
            "width": 4000, "height": 3000, "pix_fmt": "yuvj420p" }],
            "format": { "format_name": "image2" } }"#,
        )
    }

    fn transparent_png() -> MediaInfo {
        media(
            r#"{ "streams": [{ "index": 0, "codec_name": "png", "codec_type": "video",
            "width": 64, "height": 48, "pix_fmt": "rgba" }], "format": { "format_name": "png_pipe" } }"#,
        )
    }

    #[test]
    #[rustfmt::skip]
    fn jpeg_to_every_picture_format() {
        assert_eq!(
            picture_body(&jpeg(), &req(TargetFormat::Webp, QualityPreset::Balanced), "a.jpg"),
            (strings(&["-filter_complex", "[0:0]null[img]", "-map", "[img]", "-frames:v", "1",
                "-c:v", "libwebp", "-quality", "82", "-pix_fmt", "yuv420p"]), "webp".into())
        );
        assert_eq!(
            picture_body(&jpeg(), &req(TargetFormat::Png, QualityPreset::Small), "a.jpg"),
            (strings(&["-filter_complex", "[0:0]null[img]", "-map", "[img]", "-frames:v", "1",
                "-c:v", "png", "-compression_level", "9"]), "image2pipe".into())
        );
        assert_eq!(
            picture_body(&jpeg(), &req(TargetFormat::Jpg, QualityPreset::Max), "a.jpg"),
            (strings(&["-filter_complex", "[0:0]null[img]", "-map", "[img]", "-frames:v", "1",
                "-c:v", "mjpeg", "-q:v", "2", "-pix_fmt", "yuv444p", "-color_range", "pc"]), "image2pipe".into())
        );
        let mut request = req(TargetFormat::Avif, QualityPreset::Balanced);
        request.advanced.max_dimension = Some(1000);
        assert_eq!(
            picture_body(&jpeg(), &request, "a.jpg"),
            (strings(&["-filter_complex", "[0:0]scale=1000:750:flags=lanczos,format=yuv420p[img]",
                "-map", "[img]", "-frames:v", "1",
                "-c:v", "libaom-av1", "-still-picture", "1", "-cpu-used", "6", "-crf", "25", "-b:v", "0"]),
             "avif".into())
        );
    }

    #[test]
    #[rustfmt::skip]
    fn transparency_is_flattened_for_jpeg_and_split_for_avif() {
        let (args, _) = picture_body(&transparent_png(), &req(TargetFormat::Jpg, QualityPreset::Balanced), "a.png");
        assert_eq!(&args[..2], ["-filter_complex",
            "[0:0]format=rgba,split[fg][back];[back]drawbox=c=white@1:t=fill:replace=1,format=rgb24[bg];\
             [bg][fg]overlay=format=auto[img]"]);
        assert!(args.ends_with(&strings(&["-c:v", "mjpeg", "-q:v", "4", "-pix_fmt", "yuv420p", "-color_range", "pc"])));

        let (args, _) = picture_body(&transparent_png(), &req(TargetFormat::Avif, QualityPreset::Small), "a.png");
        assert_eq!(&args[..7], ["-filter_complex",
            "[0:0]format=yuva420p,split[color][a];[a]alphaextract[alpha];[color]format=yuv420p[img]",
            "-map", "[img]", "-map", "[alpha]", "-frames:v"]);
        assert!(args.windows(2).any(|w| w == ["-crf", "35"]));

        let (args, _) = picture_body(&transparent_png(), &req(TargetFormat::Webp, QualityPreset::Max), "a.png");
        assert!(args.ends_with(&strings(&["-c:v", "libwebp", "-lossless", "1", "-compression_level", "4"])));
        let (args, _) = picture_body(&transparent_png(), &req(TargetFormat::Webp, QualityPreset::Small), "a.png");
        assert!(args.ends_with(&strings(&["-quality", "65", "-pix_fmt", "yuva420p"])));
    }

    #[test]
    #[rustfmt::skip]
    fn heic_grids_and_avif_alpha_planes_are_assembled() {
        let heic = media(
            r#"{ "stream_groups": [{ "index": 0, "type": "Tile Grid", "disposition": { "default": 1 },
                "components": [{ "width": 512, "height": 384,
                  "subcomponents": [{ "stream_index": 0 }, { "stream_index": 1 }] }] }],
              "streams": [
                { "index": 0, "codec_name": "hevc", "codec_type": "video", "width": 256, "height": 384 },
                { "index": 1, "codec_name": "hevc", "codec_type": "video", "width": 256, "height": 384 }],
              "format": { "format_name": "mov,mp4,m4a,3gp,3g2,mj2", "tags": { "major_brand": "heic" } } }"#,
        );
        let (args, _) = picture_body(&heic, &req(TargetFormat::Jpg, QualityPreset::Balanced), "IMG_1.HEIC");
        assert_eq!(&args[..2], ["-filter_complex", "[0:g:0]null[img]"]);

        let avif = media(
            r#"{ "streams": [
                { "index": 0, "codec_name": "av1", "codec_type": "video", "width": 64, "height": 48,
                  "pix_fmt": "yuv420p", "disposition": { "default": 1 } },
                { "index": 1, "codec_name": "av1", "codec_type": "video", "width": 64, "height": 48,
                  "pix_fmt": "gray" }],
              "format": { "format_name": "mov,mp4,m4a,3gp,3g2,mj2", "tags": { "major_brand": "avif" } } }"#,
        );
        let (args, _) = picture_body(&avif, &req(TargetFormat::Png, QualityPreset::Balanced), "a.avif");
        assert_eq!(&args[..4], ["-filter_complex", "[0:0][0:1]alphamerge[img]", "-map", "[img]"]);
    }

    #[test]
    #[rustfmt::skip]
    fn icons_bmp_tiff_and_still_gif() {
        let (args, muxer) = picture_body(&jpeg(), &req(TargetFormat::Ico, QualityPreset::Balanced), "a.jpg");
        assert_eq!(muxer, "ico");
        assert_eq!(args, strings(&[
            "-filter_complex",
            "[0:0]format=rgba,split=4[i0][i1][i2][i3];\
             [i0]scale=16:12:flags=lanczos,pad=16:16:0:2:color=black@0[s0];\
             [i1]scale=32:24:flags=lanczos,pad=32:32:0:4:color=black@0[s1];\
             [i2]scale=48:36:flags=lanczos,pad=48:48:0:6:color=black@0[s2];\
             [i3]scale=256:192:flags=lanczos,pad=256:256:0:32:color=black@0[s3]",
            "-map", "[s0]", "-map", "[s1]", "-map", "[s2]", "-map", "[s3]",
            "-frames:v", "1", "-c:v", "png", "-compression_level", "9",
        ]));

        let (args, muxer) = picture_body(&transparent_png(), &req(TargetFormat::StillGif, QualityPreset::Max), "a.png");
        assert_eq!(muxer, "gif");
        assert_eq!(&args[..2], ["-filter_complex",
            "[0:0]split[g0][g1];[g0]palettegen=reserve_transparent=1:stats_mode=single[p];\
             [g1][p]paletteuse:alpha_threshold=128[img]"]);
        assert!(args.ends_with(&strings(&["-c:v", "gif"])));

        let (args, _) = picture_body(&transparent_png(), &req(TargetFormat::Bmp, QualityPreset::Max), "a.png");
        assert!(args[1].contains("drawbox=c=white"));
        assert!(args.ends_with(&strings(&["-c:v", "bmp", "-pix_fmt", "bgr24"])));
        let (args, _) = picture_body(&transparent_png(), &req(TargetFormat::Tiff, QualityPreset::Max), "a.png");
        assert!(args.ends_with(&strings(&["-c:v", "tiff", "-compression_algo", "deflate", "-pix_fmt", "rgba"])));
    }

    #[test]
    #[rustfmt::skip]
    fn frames_seek_or_sample_the_video() {
        use crate::request::FrameOptions;
        let mut request = req(TargetFormat::Jpg, QualityPreset::Balanced);
        request.frames = Some(FrameOptions { at_secs: Some(12.5), ..FrameOptions::default() });
        let cmd = build_command(&mp4(), &request, Path::new("clip.mp4"), Path::new("f.jpg"))
            .unwrap()
            .args_lossy();
        let i = cmd.iter().position(|a| a == "-i").unwrap();
        assert_eq!(cmd[i - 2..i], ["-ss", "12.5"]);
        assert!(cmd.windows(2).any(|w| w == ["-frames:v", "1"]));
        assert!(cmd.windows(2).any(|w| w == ["-f", "image2pipe"]));

        request.target = TargetFormat::Png;
        request.frames = Some(FrameOptions {
            mode: FrameMode::Sequence,
            every_secs: Some(2.0),
            ..FrameOptions::default()
        });
        let (args, muxer) = picture_body(&mp4(), &request, "clip.mp4");
        assert_eq!(muxer, "image2");
        assert_eq!(args, strings(&["-filter_complex", "[0:0]fps=1/2[img]", "-map", "[img]",
            "-fps_mode", "passthrough", "-c:v", "png", "-compression_level", "6"]));
        let cmd = build_command(&mp4(), &request, Path::new("clip.mp4"), Path::new("f.png"))
            .unwrap()
            .args_lossy();
        assert!(!cmd.contains(&"-ss".to_owned()));
    }

    #[test]
    #[rustfmt::skip]
    fn jpeg_xl_uses_a_distance() {
        let (args, muxer) = picture_body(&jpeg(), &req(TargetFormat::Jxl, QualityPreset::Balanced), "a.jpg");
        assert_eq!(muxer, "image2pipe");
        assert!(args.ends_with(&strings(&["-c:v", "libjxl", "-distance", "1", "-effort", "7", "-pix_fmt", "rgb24"])));
        // A lossless source at the best quality stays lossless, with transparency.
        let (args, _) = picture_body(&transparent_png(), &req(TargetFormat::Jxl, QualityPreset::Max), "a.png");
        assert!(args.ends_with(&strings(&["-c:v", "libjxl", "-distance", "0", "-effort", "7", "-pix_fmt", "rgba"])));
        assert_eq!([100, 95, 90, 75, 30, 1].map(jxl_distance), [0.0, 0.55, 1.0, 2.35, 6.4, 23.87]);
    }

    #[test]
    fn picture_quality_scales() {
        assert_eq!(
            [100, 95, 90, 85, 70, 50, 1].map(jpeg_qscale),
            [2, 2, 3, 4, 7, 11, 21]
        );
        assert_eq!([100, 75, 60, 45, 1].map(avif_crf), [0, 16, 25, 35, 62]);
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
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
