//! Media analysis with FFprobe.
//!
//! [`parse_ffprobe_json`] is a pure function (unit-tested with captured output);
//! [`probe`] runs FFprobe on a local file and feeds its JSON to the parser.
//! Still pictures need a second look at their first frame
//! ([`apply_first_frame_json`]): rotation and EXIF data live there.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::binaries::Binaries;
use crate::error::CoreError;
use crate::process;

/// What kind of content a file holds, which drives the proposed targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MediaKind {
    /// Audio only (cover art pictures do not count as video).
    Audio,
    /// Video without any audio track.
    Video,
    /// Video with at least one audio track.
    VideoWithAudio,
    /// A single picture (photo): JPEG, PNG, WebP, AVIF, HEIC, BMP, TIFF, ...
    Image,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioStream {
    pub index: u32,
    pub codec: String,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub bit_rate: Option<u64>,
    /// Bits per sample for PCM/lossless sources (e.g. 16, 24).
    pub bit_depth: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VideoStream {
    pub index: u32,
    pub codec: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub bit_rate: Option<u64>,
    pub pix_fmt: Option<String>,
}

/// An embedded cover picture (album art).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CoverArt {
    pub index: u32,
    /// Picture codec, usually `mjpeg` or `png`.
    pub codec: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

impl CoverArt {
    /// Pictures that MP3/M4A/FLAC can store as-is.
    pub fn is_portable(&self) -> bool {
        matches!(self.codec.as_str(), "mjpeg" | "png")
    }
}

/// Details of a still picture ([`MediaKind::Image`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PictureInfo {
    /// The picture has, or may have, transparent areas.
    pub alpha: bool,
    /// Stream holding a separate transparency plane (AVIF).
    pub alpha_stream: Option<u32>,
    /// Stream group of a picture stored as a grid of tiles (HEIC/AVIF photos).
    pub grid_group: Option<u32>,
    /// Embedded EXIF data (camera, date, location, ...).
    pub exif: bool,
}

/// Result of analysing an input file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaInfo {
    /// FFmpeg demuxer name(s), e.g. `wav` or `mov,mp4,m4a,3gp,3g2,mj2`.
    pub container: String,
    pub kind: MediaKind,
    pub duration_secs: Option<f64>,
    pub size_bytes: Option<u64>,
    /// Overall bit rate in bits per second.
    pub bit_rate: Option<u64>,
    pub audio: Vec<AudioStream>,
    /// Real video streams (attached cover pictures excluded), with their
    /// displayed (rotated) size. For a picture: only the picture itself.
    pub video: Vec<VideoStream>,
    /// Embedded cover picture (e.g. album art in MP3/M4A/FLAC).
    pub cover_art: Option<CoverArt>,
    pub subtitle_streams: u32,
    /// Picture details, for [`MediaKind::Image`] only.
    #[serde(default)]
    pub picture: Option<PictureInfo>,
    /// The file has tags (title, artist, album...).
    #[serde(default)]
    pub has_tags: bool,
}

impl MediaInfo {
    pub fn primary_audio(&self) -> Option<&AudioStream> {
        self.audio.first()
    }

    pub fn primary_video(&self) -> Option<&VideoStream> {
        self.video.first()
    }

    /// The input is a HEIF/AVIF container (brand-based picture in an ISO file).
    pub fn is_heif(&self) -> bool {
        self.kind == MediaKind::Image && self.container.starts_with("mov,")
    }
}

/// Runs FFprobe on a local file. Network protocols are not allowed.
pub async fn probe(bins: &Binaries, input: &Path) -> Result<MediaInfo, CoreError> {
    if !input.is_file() {
        return Err(CoreError::InputNotFound);
    }
    let json = run_ffprobe(
        bins,
        input,
        &["-show_format", "-show_streams", "-show_stream_groups"],
    )
    .await?;
    let mut info = parse_ffprobe_json(&json)?;
    if info.kind == MediaKind::Image && !info.is_heif() {
        // EXIF orientation and data are only visible on the decoded picture
        // (HEIF/AVIF files declare them in the container instead). Icons hold
        // several pictures: look at the one chosen as the main picture.
        let stream = info.video.first().map_or(0, |v| v.index).to_string();
        let json = run_ffprobe(
            bins,
            input,
            &[
                "-select_streams",
                &stream,
                "-read_intervals",
                "%+#1",
                "-show_entries",
                "frame=width,height,pix_fmt:frame_side_data=side_data_type,rotation",
            ],
        )
        .await?;
        apply_first_frame_json(&mut info, &json)?;
    }
    Ok(info)
}

async fn run_ffprobe(bins: &Binaries, input: &Path, args: &[&str]) -> Result<Vec<u8>, CoreError> {
    let mut cmd = process::engine_command(&bins.ffprobe);
    cmd.args(["-hide_banner", "-v", "error"])
        .args(["-protocol_whitelist", "file"])
        .args(["-print_format", "json"])
        .args(args)
        .arg(crate::command::file_url(input));
    let output = process::spawn(&mut cmd)?.wait_with_output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(match CoreError::from_engine_output(&stderr) {
            CoreError::EngineFailed { details } => CoreError::CorruptInput { details },
            other => other,
        });
    }
    Ok(output.stdout)
}

fn unexpected_output(e: serde_json::Error) -> CoreError {
    CoreError::CorruptInput {
        details: format!("unexpected ffprobe output: {e}"),
    }
}

/// Converts FFprobe's `-print_format json -show_format -show_streams
/// -show_stream_groups` output.
pub fn parse_ffprobe_json(json: &[u8]) -> Result<MediaInfo, CoreError> {
    let raw: RawProbe = serde_json::from_slice(json).map_err(unexpected_output)?;
    let format = raw.format.clone().unwrap_or_default();
    let container = format.format_name.clone().unwrap_or_default();
    let brand = format.tags.get("major_brand").map_or("", String::as_str);

    let mut audio = Vec::new();
    let mut video = Vec::new();
    let mut cover_art = None;
    let mut subtitle_streams = 0;

    for s in raw.streams.iter().cloned() {
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
            Some("video") if s.is_attached_picture() => {
                cover_art.get_or_insert(CoverArt {
                    index: s.index,
                    codec,
                    width: s.width,
                    height: s.height,
                });
            }
            Some("video") => {
                // Phone videos are often stored sideways with a display matrix:
                // FFmpeg rotates them before any filter, so plans need the
                // displayed size.
                let (width, height) = if s.side_data_list.iter().any(RawSideData::is_quarter_turn) {
                    (s.height, s.width)
                } else {
                    (s.width, s.height)
                };
                video.push(VideoStream {
                    index: s.index,
                    codec,
                    width,
                    height,
                    fps: parse_rate(s.avg_frame_rate.as_deref())
                        .or_else(|| parse_rate(s.r_frame_rate.as_deref())),
                    bit_rate: parse_num(&s.bit_rate),
                    pix_fmt: s.pix_fmt,
                })
            }
            Some("subtitle") => subtitle_streams += 1,
            _ => {}
        }
    }

    // Ogg and FLAC keep their comments on the audio stream, others on the file.
    let has_tags = format
        .tags
        .keys()
        .chain(
            raw.streams
                .iter()
                .filter(|s| s.codec_type.as_deref() == Some("audio"))
                .flat_map(|s| s.tags.keys()),
        )
        .any(|key| USER_TAGS.iter().any(|t| key.eq_ignore_ascii_case(t)));

    if audio.is_empty() && is_still_picture(&container, brand, &video) {
        return parse_picture(&raw, container, parse_num(&format.size));
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
        cover_art,
        subtitle_streams,
        picture: None,
        has_tags,
    })
}

/// Tags people care about (technical ones such as `encoder` do not count).
const USER_TAGS: &[&str] = &[
    "title",
    "artist",
    "album",
    "album_artist",
    "composer",
    "genre",
    "date",
    "track",
    "disc",
    "comment",
];

/// Brands of HEIF/AVIF files holding a still picture (not a picture sequence).
const PICTURE_BRANDS: &[&str] = &["avif", "heic", "heix", "heim", "heis", "mif1", "mif2"];

/// A single picture: image demuxers (JPEG, PNG, WebP, BMP, TIFF, ...) and
/// HEIF/AVIF files. Animated GIF, APNG and WebP stay videos.
fn is_still_picture(container: &str, brand: &str, video: &[VideoStream]) -> bool {
    let image_demuxer = matches!(container, "image2" | "ico") || container.ends_with("_pipe");
    let heif = container.starts_with("mov,") && PICTURE_BRANDS.contains(&brand);
    let animated = video
        .iter()
        .any(|v| matches!(v.codec.as_str(), "gif" | "apng" | "webp_anim"));
    !video.is_empty() && (image_demuxer || heif) && !animated
}

/// Picture described by its main stream, or by the tile grid it is made of.
fn parse_picture(
    raw: &RawProbe,
    container: String,
    size_bytes: Option<u64>,
) -> Result<MediaInfo, CoreError> {
    let streams: Vec<&RawStream> = raw
        .streams
        .iter()
        .filter(|s| s.codec_type.as_deref() == Some("video") && !s.is_attached_picture())
        .collect();
    let stream_by_index = |index: u32| streams.iter().copied().find(|s| s.index == index);

    let grids: Vec<RawStreamGroup> = raw
        .stream_groups
        .iter()
        .filter(|g| g.get("type").and_then(|t| t.as_str()) == Some("Tile Grid"))
        .filter_map(|g| serde_json::from_value(g.clone()).ok())
        .collect();
    let grid = grids
        .iter()
        .find(|g| g.is_default())
        .or(grids.first())
        .and_then(|g| Some((g, g.components.first()?)));

    let mut picture = PictureInfo::default();
    let (main, width, height, side_data) = match grid {
        Some((group, component)) => {
            let first_tile = component
                .subcomponents
                .iter()
                .find_map(|t| t.stream_index.and_then(stream_by_index))
                .ok_or(CoreError::NotMedia)?;
            picture.grid_group = Some(group.index);
            let side_data: Vec<&RawSideData> = component
                .side_data_list
                .iter()
                .chain(&group.side_data_list)
                .collect();
            (first_tile, component.width, component.height, side_data)
        }
        None => {
            let largest = || {
                streams
                    .iter()
                    .copied()
                    .max_by_key(|s| s.width.unwrap_or(0) as u64 * s.height.unwrap_or(0) as u64)
            };
            let main = if container == "ico" {
                // An icon holds the same picture in several sizes.
                largest()
            } else {
                streams
                    .iter()
                    .copied()
                    .find(|s| s.is_default() && !s.is_dependent())
                    .or(streams.first().copied())
            }
            .ok_or(CoreError::NotMedia)?;
            // A grayscale twin of the picture is its transparency plane (AVIF).
            picture.alpha_stream = streams
                .iter()
                .find(|s| {
                    s.index != main.index
                        && s.codec_name == main.codec_name
                        && s.pix_fmt.as_deref().is_some_and(|p| p.starts_with("gray"))
                        && (s.width, s.height) == (main.width, main.height)
                })
                .map(|s| s.index);
            (
                main,
                main.width,
                main.height,
                main.side_data_list.iter().collect(),
            )
        }
    };

    picture.exif = side_data.iter().any(|d| d.is_exif());
    picture.alpha =
        picture.alpha_stream.is_some() || main.pix_fmt.as_deref().is_some_and(pix_fmt_has_alpha);
    let mut stream = VideoStream {
        index: main.index,
        codec: main.codec_name.clone().unwrap_or_default(),
        width,
        height,
        fps: None,
        bit_rate: None,
        pix_fmt: main.pix_fmt.clone(),
    };
    if side_data.iter().any(|d| d.is_quarter_turn()) {
        std::mem::swap(&mut stream.width, &mut stream.height);
    }

    Ok(MediaInfo {
        container,
        kind: MediaKind::Image,
        duration_secs: None,
        size_bytes,
        bit_rate: None,
        audio: Vec::new(),
        video: vec![stream],
        cover_art: None,
        subtitle_streams: 0,
        picture: Some(picture),
        has_tags: false,
    })
}

/// Completes a picture with its first decoded frame (`-show_entries
/// frame=width,height,pix_fmt:frame_side_data=side_data_type,rotation`):
/// EXIF data and the EXIF orientation, which FFmpeg applies when converting.
pub fn apply_first_frame_json(info: &mut MediaInfo, json: &[u8]) -> Result<(), CoreError> {
    let raw: RawFrames = serde_json::from_slice(json).map_err(unexpected_output)?;
    let frame = raw.frames.first().ok_or_else(|| CoreError::CorruptInput {
        details: "the picture could not be decoded".into(),
    })?;
    let picture = info.picture.get_or_insert_default();
    picture.exif |= frame.side_data_list.iter().any(RawSideData::is_exif);
    if let Some(stream) = info.video.first_mut() {
        if frame.width.is_some() && frame.height.is_some() {
            (stream.width, stream.height) = (frame.width, frame.height);
        }
        if frame
            .side_data_list
            .iter()
            .any(RawSideData::is_quarter_turn)
        {
            std::mem::swap(&mut stream.width, &mut stream.height);
        }
        if let Some(pix_fmt) = &frame.pix_fmt {
            picture.alpha |= pix_fmt_has_alpha(pix_fmt);
            stream.pix_fmt = Some(pix_fmt.clone());
        }
    }
    Ok(())
}

/// Pixel formats with an alpha channel (`pal8`: the palette may have one).
pub(crate) fn pix_fmt_has_alpha(pix_fmt: &str) -> bool {
    pix_fmt == "pal8"
        || ["yuva", "gbrap", "ya", "rgba", "bgra", "argb", "abgr"]
            .iter()
            .any(|prefix| pix_fmt.starts_with(prefix))
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
    /// Kept loose: only tile grids are parsed, other group types are ignored.
    #[serde(default)]
    stream_groups: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawFormat {
    format_name: Option<String>,
    duration: Option<String>,
    size: Option<String>,
    bit_rate: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

/// A `Tile Grid` stream group (HEIF/AVIF picture split into tiles).
#[derive(Debug, Deserialize)]
struct RawStreamGroup {
    index: u32,
    #[serde(default)]
    components: Vec<RawGridComponent>,
    #[serde(default)]
    side_data_list: Vec<RawSideData>,
    #[serde(default)]
    disposition: HashMap<String, i64>,
}

impl RawStreamGroup {
    fn is_default(&self) -> bool {
        self.disposition.get("default").copied() == Some(1)
    }
}

#[derive(Debug, Deserialize)]
struct RawGridComponent {
    width: Option<u32>,
    height: Option<u32>,
    #[serde(default)]
    subcomponents: Vec<RawTile>,
    #[serde(default)]
    side_data_list: Vec<RawSideData>,
}

#[derive(Debug, Deserialize)]
struct RawTile {
    stream_index: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawSideData {
    side_data_type: Option<String>,
    /// Display rotation in degrees (counter-clockwise).
    rotation: Option<f64>,
}

impl RawSideData {
    fn is_exif(&self) -> bool {
        self.side_data_type.as_deref() == Some("EXIF metadata")
    }

    /// Rotated by 90° or 270°: width and height are swapped on display.
    fn is_quarter_turn(&self) -> bool {
        self.rotation
            .is_some_and(|r| (r.round() as i64).rem_euclid(180) == 90)
    }
}

#[derive(Debug, Deserialize)]
struct RawFrames {
    #[serde(default)]
    frames: Vec<RawFrame>,
}

#[derive(Debug, Deserialize)]
struct RawFrame {
    width: Option<u32>,
    height: Option<u32>,
    pix_fmt: Option<String>,
    #[serde(default)]
    side_data_list: Vec<RawSideData>,
}

#[derive(Debug, Clone, Deserialize)]
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
    #[serde(default)]
    side_data_list: Vec<RawSideData>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

impl RawStream {
    fn flag(&self, name: &str) -> bool {
        self.disposition.get(name).copied() == Some(1)
    }

    fn is_attached_picture(&self) -> bool {
        self.flag("attached_pic")
    }

    fn is_default(&self) -> bool {
        self.flag("default")
    }

    /// Part of something else (e.g. one tile of a HEIC grid).
    fn is_dependent(&self) -> bool {
        self.flag("dependent")
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
        let cover = info.cover_art.as_ref().unwrap();
        assert_eq!((cover.index, cover.codec.as_str()), (1, "mjpeg"));
        assert!(cover.is_portable());
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
    fn sideways_phone_video_reports_its_displayed_size() {
        let json = r#"{ "streams": [{ "index": 0, "codec_name": "h264", "codec_type": "video",
            "width": 1920, "height": 1080, "avg_frame_rate": "30/1",
            "side_data_list": [{ "side_data_type": "Display Matrix", "rotation": -90 }] }],
            "format": { "format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "5" } }"#;
        let info = parse_ffprobe_json(json.as_bytes()).unwrap();
        assert_eq!(info.kind, MediaKind::Video);
        let v = info.primary_video().unwrap();
        assert_eq!((v.width, v.height), (Some(1080), Some(1920)));

        // Upside down: same size.
        let json = json.replace("-90", "180");
        let v = parse_ffprobe_json(json.as_bytes()).unwrap().video[0].clone();
        assert_eq!((v.width, v.height), (Some(1920), Some(1080)));
    }

    #[test]
    fn empty_files_are_not_media() {
        let empty = r#"{ "streams": [], "format": { "format_name": "mp3" } }"#;
        assert!(matches!(
            parse_ffprobe_json(empty.as_bytes()),
            Err(CoreError::NotMedia)
        ));
    }

    #[test]
    fn png_is_a_picture_with_transparency() {
        let png = r#"{ "streams": [{ "index": 0, "codec_name": "png", "codec_type": "video",
            "width": 10, "height": 20, "pix_fmt": "rgba", "avg_frame_rate": "25/1" }],
            "format": { "format_name": "png_pipe", "size": "1234" } }"#;
        let info = parse_ffprobe_json(png.as_bytes()).unwrap();
        assert_eq!(info.kind, MediaKind::Image);
        assert_eq!(info.duration_secs, None);
        assert_eq!(info.size_bytes, Some(1234));
        let v = info.primary_video().unwrap();
        assert_eq!((v.width, v.height, v.fps), (Some(10), Some(20), None));
        let picture = info.picture.unwrap();
        assert!(picture.alpha && !picture.exif);
        assert_eq!((picture.grid_group, picture.alpha_stream), (None, None));
    }

    #[test]
    fn exif_orientation_of_the_first_frame_rotates_the_picture() {
        let jpeg = r#"{ "streams": [{ "index": 0, "codec_name": "mjpeg", "codec_type": "video",
            "width": 640, "height": 480, "pix_fmt": "yuvj420p" }],
            "format": { "format_name": "image2", "duration": "0.04" } }"#;
        let mut info = parse_ffprobe_json(jpeg.as_bytes()).unwrap();
        assert_eq!(info.kind, MediaKind::Image);
        assert!(!info.is_heif());
        let frame = r#"{ "frames": [{ "width": 640, "height": 480, "pix_fmt": "yuvj420p",
            "side_data_list": [
                { "side_data_type": "3x3 displaymatrix", "rotation": -90 },
                { "side_data_type": "EXIF metadata" } ] }] }"#;
        apply_first_frame_json(&mut info, frame.as_bytes()).unwrap();
        let v = info.primary_video().unwrap();
        assert_eq!((v.width, v.height), (Some(480), Some(640)));
        let picture = info.picture.unwrap();
        assert!(picture.exif && !picture.alpha);

        let mut info = parse_ffprobe_json(jpeg.as_bytes()).unwrap();
        let err = apply_first_frame_json(&mut info, br#"{ "frames": [] }"#).unwrap_err();
        assert_eq!(err.code(), "corrupt_input");
    }

    #[test]
    fn heic_tile_grid_is_one_rotated_picture() {
        let tile = |i: u32| {
            format!(
                r#"{{ "index": {i}, "codec_name": "hevc", "codec_type": "video", "width": 256,
                "height": 192, "pix_fmt": "yuv420p", "disposition": {{ "default": 0, "dependent": 1 }} }}"#
            )
        };
        let heic = format!(
            r#"{{ "stream_groups": [{{ "index": 0, "type": "Tile Grid",
                "disposition": {{ "default": 1 }},
                "components": [{{ "width": 512, "height": 384,
                  "subcomponents": [{{ "stream_index": 0 }}, {{ "stream_index": 1 }},
                                    {{ "stream_index": 2 }}, {{ "stream_index": 3 }}],
                  "side_data_list": [{{ "side_data_type": "EXIF metadata", "size": 34 }},
                                     {{ "side_data_type": "Display Matrix", "rotation": 90 }}] }}] }},
                {{ "index": 1, "type": "IAMF Audio Element", "components": [{{ "subcomponents": [{{}}] }}] }}],
              "streams": [{}, {}, {}, {},
                {{ "index": 4, "codec_name": "hevc", "codec_type": "video", "width": 320, "height": 240,
                  "pix_fmt": "yuv420p", "disposition": {{ "default": 0 }} }}],
              "format": {{ "format_name": "mov,mp4,m4a,3gp,3g2,mj2",
                "tags": {{ "major_brand": "heic", "compatible_brands": "mif1heic" }} }} }}"#,
            tile(0),
            tile(1),
            tile(2),
            tile(3)
        );
        let info = parse_ffprobe_json(heic.as_bytes()).unwrap();
        assert_eq!(info.kind, MediaKind::Image);
        assert!(info.is_heif());
        let v = info.primary_video().unwrap();
        assert_eq!(info.video.len(), 1);
        assert_eq!((v.index, v.codec.as_str()), (0, "hevc"));
        assert_eq!((v.width, v.height), (Some(384), Some(512)));
        let picture = info.picture.unwrap();
        assert_eq!(picture.grid_group, Some(0));
        assert!(picture.exif && !picture.alpha);
    }

    #[test]
    fn avif_alpha_plane_is_detected() {
        let avif = r#"{ "streams": [
            { "index": 0, "codec_name": "av1", "codec_type": "video", "width": 64, "height": 48,
              "pix_fmt": "yuv420p", "disposition": { "default": 1 } },
            { "index": 1, "codec_name": "av1", "codec_type": "video", "width": 64, "height": 48,
              "pix_fmt": "gray", "disposition": { "default": 0 } }],
            "format": { "format_name": "mov,mp4,m4a,3gp,3g2,mj2", "tags": { "major_brand": "avif" } } }"#;
        let info = parse_ffprobe_json(avif.as_bytes()).unwrap();
        assert_eq!(info.kind, MediaKind::Image);
        let picture = info.picture.unwrap();
        assert_eq!(picture.alpha_stream, Some(1));
        assert!(picture.alpha);
    }

    #[test]
    fn animations_and_picture_sequences_stay_videos() {
        for (container, codec, brand) in [
            ("webp_anim", "webp_anim", ""),
            ("apng", "apng", ""),
            ("gif", "gif", ""),
            ("mov,mp4,m4a,3gp,3g2,mj2", "av1", "avis"),
            ("mov,mp4,m4a,3gp,3g2,mj2", "hevc", "msf1"),
        ] {
            let json = format!(
                r#"{{ "streams": [{{ "index": 0, "codec_name": "{codec}", "codec_type": "video",
                "width": 32, "height": 32 }}],
                "format": {{ "format_name": "{container}", "tags": {{ "major_brand": "{brand}" }} }} }}"#
            );
            let info = parse_ffprobe_json(json.as_bytes()).unwrap();
            assert_eq!(info.kind, MediaKind::Video, "{container} {brand}");
            assert_eq!(info.picture, None);
        }
    }

    #[test]
    fn garbage_is_reported_as_corrupt() {
        assert!(matches!(
            parse_ffprobe_json(b"not json"),
            Err(CoreError::CorruptInput { .. })
        ));
    }
}
