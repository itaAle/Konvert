//! Smaller copies of files in their own format ("Comprimi").
//!
//! The user picks a level and maybe a size limit; Konvert turns them into the
//! ordinary choices of a conversion into the same format: a lower quality,
//! bit rate, sample rate or resolution, fewer colors ([`Attempt`]). With a
//! limit, [`Search`] tries stronger settings until the file fits. PDFs are
//! compressed by [`crate::pdf_compress`], without FFmpeg.
//!
//! A smaller copy is never larger than its source: the runner checks the
//! result and reports [`CoreError::NoGain`] instead.

use std::collections::VecDeque;
use std::path::Path;

use crate::error::CoreError;
use crate::estimate::estimate_size;
use crate::formats::{self, AudioCodec, FormatKind, ImageCodec, TargetFormat, VideoCodec};
use crate::plan::{self, ConversionPlan, ConversionWarning, ImageQuality};
use crate::probe::{MediaInfo, MediaKind};
use crate::request::{
    AdvancedOptions, ChannelLayout, CompressLevel, CompressOptions, ConversionRequest,
    QualityPreset,
};

/// Format of a smaller copy of this input: its own, when Konvert knows how to
/// make it smaller (`None` otherwise: BMP, TIFF, icons, HEIC, SVG...).
pub fn compress_format(media: &MediaInfo, input: &Path) -> Option<TargetFormat> {
    match media.kind {
        MediaKind::Document => return Some(TargetFormat::Pdf),
        MediaKind::Text => return Some(TargetFormat::Docx),
        _ => {}
    }
    let format = formats::source_format(media, input)?;
    let spec = formats::spec(format);
    let compressible = match (spec.kind, media.kind) {
        (FormatKind::Image, MediaKind::Image) => matches!(
            spec.image_codec,
            Some(
                ImageCodec::Jpeg
                    | ImageCodec::Png
                    | ImageCodec::Webp
                    | ImageCodec::Avif
                    | ImageCodec::Jxl
            )
        ),
        (FormatKind::Video, MediaKind::Video | MediaKind::VideoWithAudio) => {
            media.primary_video().is_some()
        }
        (FormatKind::Audio, MediaKind::Audio) => media.primary_audio().is_some(),
        _ => false,
    };
    compressible.then_some(format)
}

/// Plans a smaller copy: the first attempt, as the user sees it before starting.
pub(crate) fn plan(
    media: &MediaInfo,
    request: &ConversionRequest,
    options: &CompressOptions,
    input: &Path,
) -> Result<ConversionPlan, CoreError> {
    let format = compress_format(media, input)
        .filter(|f| *f == request.target)
        .ok_or_else(|| CoreError::UnsupportedConversion {
            details: format!(
                "a smaller {} cannot be made from this file",
                formats::spec(request.target).label
            ),
        })?;
    if format == TargetFormat::Pdf || format == TargetFormat::Docx {
        return Ok(ConversionPlan {
            target: TargetFormat::Pdf,
            video_stream: None,
            video: None,
            audio_stream: None,
            audio: None,
            cover_art_stream: None,
            image: None,
            frames: None,
            document: None,
            remove_location: false,
            compress: Some(options.clone()),
            tracks: Vec::new(),
            trim: None,
            extract: Vec::new(),
            warnings: if options.pages_as_pictures && format == TargetFormat::Pdf {
                vec![ConversionWarning::PdfPagesAsPictures]
            } else {
                Vec::new()
            },
            // How much the pictures of a PDF shrink is only known once done.
            estimate: None,
        });
    }
    let search = Search::new(media, options, format, request.remove_location);
    let mut plan = search.first().plan(media, input)?;
    plan.compress = Some(options.clone());
    if let Some(estimate) = plan.estimate.as_mut() {
        let limits = [options.max_bytes, media.size_bytes];
        if let Some(limit) = limits.into_iter().flatten().min() {
            estimate.bytes = estimate.bytes.min(limit);
        }
    }
    Ok(plan)
}

/// One way of making the smaller copy: an ordinary conversion into the same
/// format, plus what such a request cannot say.
#[derive(Debug, Clone, PartialEq)]
pub struct Attempt {
    pub request: ConversionRequest,
    /// PNG: at most 256 colors (much smaller, slightly grainy).
    pub palette: bool,
    /// The photo's EXIF data is left out.
    pub drop_exif: bool,
}

impl Attempt {
    pub fn plan(&self, media: &MediaInfo, input: &Path) -> Result<ConversionPlan, CoreError> {
        let mut plan = plan::plan_offered(media, &self.request, input)?;
        if let Some(image) = plan.image.as_mut() {
            if self.palette && image.codec == ImageCodec::Png {
                image.quality = ImageQuality::Palette;
            }
            if self.drop_exif {
                image.copy_exif = false;
            }
        }
        plan.warnings.retain(|w| {
            // Re-encoding is what was asked for; dropped EXIF data is the point.
            !matches!(
                w,
                ConversionWarning::LossyReencode | ConversionWarning::MetadataDropped
            )
        });
        if self.palette {
            plan.estimate = estimate_size(media, &plan);
        }
        Ok(plan)
    }
}

/// Settings tried one after the other until the file fits under the limit.
#[derive(Debug, Clone)]
pub struct Search {
    first: Attempt,
    max_bytes: Option<u64>,
    next: Next,
}

#[derive(Debug, Clone)]
enum Next {
    /// Stronger settings, in order.
    Ladder(VecDeque<Attempt>),
    /// The bit rate of the last attempt, scaled by how much it overshot.
    Rate { video: bool, retries: u32 },
}

/// Attempts at a bit rate computed from the limit (encoders do not hit it
/// exactly): the first one and at most this many corrections.
const RATE_RETRIES: u32 = 2;

impl Search {
    pub fn new(
        media: &MediaInfo,
        options: &CompressOptions,
        format: TargetFormat,
        remove_location: bool,
    ) -> Self {
        let spec = formats::spec(format);
        let level = options.level;
        let base = ConversionRequest {
            remove_location,
            ..ConversionRequest::new(format, QualityPreset::Small)
        };
        let attempt = |advanced: AdvancedOptions, palette: bool| Attempt {
            request: ConversionRequest {
                advanced,
                ..base.clone()
            },
            palette,
            drop_exif: level == CompressLevel::Max,
        };
        let limit = options.max_bytes;
        let duration = media.duration_secs.filter(|d| *d > 0.1);
        let (first, next) = match (spec.kind, spec.image_codec, spec.default_video_codec()) {
            (FormatKind::Image, Some(codec), _) => {
                let (width, height) = picture_size(media);
                let longest = width.max(height);
                let ladder = picture_ladder(codec, level, longest, has_alpha(media));
                let mut ladder: VecDeque<Attempt> = ladder
                    .into_iter()
                    .map(|step| {
                        attempt(
                            AdvancedOptions {
                                image_quality: step.quality,
                                max_dimension: step.side,
                                ..AdvancedOptions::default()
                            },
                            step.palette,
                        )
                    })
                    .collect();
                let first = ladder.pop_front().expect("a ladder has a first step");
                (first, Next::Ladder(ladder))
            }
            (FormatKind::Video, _, Some(codec)) if codec.is_animation() => {
                let mut ladder: VecDeque<Attempt> = animation_ladder(media, level)
                    .into_iter()
                    .map(|advanced| attempt(advanced, false))
                    .collect();
                let first = ladder.pop_front().expect("a ladder has a first step");
                (first, Next::Ladder(ladder))
            }
            (FormatKind::Video, _, Some(codec)) => {
                let audio = spec.default_audio_codec();
                let advanced = match (limit, duration) {
                    (Some(bytes), Some(secs)) => {
                        video_for_size(media, codec, audio, level, bytes, secs)
                    }
                    _ => video_for_level(media, codec, audio, level),
                };
                let next = Next::Rate {
                    video: true,
                    retries: RATE_RETRIES,
                };
                (attempt(advanced, false), next)
            }
            (FormatKind::Audio, _, _) => {
                let codec = spec
                    .default_audio_codec()
                    .expect("audio formats have an audio codec");
                let channels = media.primary_audio().and_then(|a| a.channels).unwrap_or(2);
                if codec.is_lossless() {
                    let rate = media.primary_audio().and_then(|a| a.sample_rate);
                    let mut ladder: VecDeque<Attempt> =
                        lossless_audio_ladder(level, rate, channels)
                            .into_iter()
                            .map(|advanced| attempt(advanced, false))
                            .collect();
                    let first = ladder.pop_front().expect("a ladder has a first step");
                    (first, Next::Ladder(ladder))
                } else {
                    let advanced = match (limit, duration) {
                        (Some(bytes), Some(secs)) => {
                            audio_for_size(codec, level, channels, bytes, secs)
                        }
                        _ => audio_for_level(codec, level, channels),
                    };
                    let next = Next::Rate {
                        video: false,
                        retries: RATE_RETRIES,
                    };
                    (attempt(advanced, false), next)
                }
            }
            _ => (
                attempt(AdvancedOptions::default(), false),
                Next::Ladder(VecDeque::new()),
            ),
        };
        Self {
            first,
            max_bytes: limit,
            next,
        }
    }

    pub fn first(&self) -> Attempt {
        self.first.clone()
    }

    /// Whether a result of `size` bytes is within the limit.
    pub fn fits(&self, size: u64) -> bool {
        self.max_bytes.is_none_or(|max| size <= max)
    }

    /// After `last` gave `size` bytes: the next attempt, if the file is still
    /// too large and something is left to try.
    pub fn next(&mut self, last: &Attempt, size: u64) -> Option<Attempt> {
        let max = self.max_bytes?;
        if size <= max {
            return None;
        }
        match &mut self.next {
            Next::Ladder(ladder) => ladder.pop_front(),
            Next::Rate { video, retries } => {
                if *retries == 0 {
                    return None;
                }
                *retries -= 1;
                // Aim a little below the limit: the encoders overshoot.
                let scale = max as f64 / size as f64 * 0.92;
                let mut next = last.clone();
                let adv = &mut next.request.advanced;
                let rate = if *video {
                    &mut adv.video_bitrate_kbps
                } else {
                    &mut adv.audio_bitrate_kbps
                };
                let old = (*rate)?;
                let floor = if *video { 30 } else { 8 };
                let new = ((old as f64 * scale) as u32).max(floor);
                if new >= old {
                    return None;
                }
                *rate = Some(new);
                if !*video {
                    let codec = formats::spec(next.request.target).default_audio_codec()?;
                    adv.audio_bitrate_kbps = Some(snap_audio_kbps(codec, new));
                    if adv.audio_bitrate_kbps == Some(old) {
                        return None;
                    }
                }
                Some(next)
            }
        }
    }
}

// --- Pictures ----------------------------------------------------------------------

/// One setting of a picture: lossy quality, longest side, PNG colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PictureStep {
    quality: Option<u8>,
    side: Option<u32>,
    palette: bool,
}

/// Longest side of the copy at each level (`None`: unchanged).
fn level_side(level: CompressLevel) -> Option<u32> {
    match level {
        CompressLevel::Light => None,
        CompressLevel::Strong => Some(2560),
        CompressLevel::Max => Some(1600),
    }
}

/// Quality of the lossy picture encoders at each level, on their own scales.
fn picture_quality(codec: ImageCodec, level: CompressLevel) -> u8 {
    let (light, strong, max) = match codec {
        ImageCodec::Avif => (55, 45, 30),
        ImageCodec::Webp => (75, 60, 40),
        _ => (80, 65, 45),
    };
    match level {
        CompressLevel::Light => light,
        CompressLevel::Strong => strong,
        CompressLevel::Max => max,
    }
}

/// Settings of a picture from the chosen level to the strongest: lower
/// qualities first, then smaller pictures. PNG keeps every pixel at the light
/// level, then uses 256 colors (with transparency, only at the strongest
/// level: smooth transparent edges would become jagged).
fn picture_ladder(
    codec: ImageCodec,
    level: CompressLevel,
    longest: u32,
    alpha: bool,
) -> Vec<PictureStep> {
    let mut steps = Vec::new();
    let level_side = level_side(level).filter(|s| *s < longest);
    let shrink = |steps: &mut Vec<PictureStep>, quality: Option<u8>, palette: bool| {
        let mut side = level_side.unwrap_or(longest);
        loop {
            side = side * 3 / 4;
            if side < 320 {
                break;
            }
            steps.push(PictureStep {
                quality,
                side: Some(side),
                palette,
            });
        }
    };
    if codec == ImageCodec::Png {
        let palette = match level {
            CompressLevel::Light => false,
            CompressLevel::Strong => !alpha,
            CompressLevel::Max => true,
        };
        steps.push(PictureStep {
            quality: None,
            side: level_side,
            palette,
        });
        if !palette {
            steps.push(PictureStep {
                quality: None,
                side: level_side,
                palette: true,
            });
        }
        shrink(&mut steps, None, true);
        return steps;
    }
    let floor = if codec == ImageCodec::Avif { 20 } else { 25 };
    let mut quality = picture_quality(codec, level);
    loop {
        steps.push(PictureStep {
            quality: Some(quality),
            side: level_side,
            palette: false,
        });
        if quality <= floor {
            break;
        }
        quality = quality.saturating_sub(10).max(floor);
    }
    shrink(
        &mut steps,
        Some(picture_quality(codec, CompressLevel::Max)),
        false,
    );
    steps
}

fn picture_size(media: &MediaInfo) -> (u32, u32) {
    media
        .primary_video()
        .and_then(|v| v.width.zip(v.height))
        .unwrap_or((0, 0))
}

fn has_alpha(media: &MediaInfo) -> bool {
    media.picture.as_ref().is_some_and(|p| p.alpha)
}

// --- Video -------------------------------------------------------------------------

/// Bits per pixel and frame of H.264 at the stronger levels (the light level
/// is the "small file" preset).
fn bits_per_pixel(level: CompressLevel) -> Option<f64> {
    match level {
        CompressLevel::Light => None,
        CompressLevel::Strong => Some(0.032),
        CompressLevel::Max => Some(0.022),
    }
}

/// Bits a codec needs for the same quality, relative to H.264 (rough).
fn efficiency(codec: VideoCodec) -> f64 {
    match codec {
        VideoCodec::Vp9 => 1.4,
        VideoCodec::Av1 => 1.8,
        VideoCodec::Mpeg4 => 0.7,
        VideoCodec::Mpeg2 => 0.5,
        _ => 1.0,
    }
}

/// Highest video picture at each level (`None`: unchanged).
fn level_height(level: CompressLevel) -> Option<u32> {
    match level {
        CompressLevel::Light => None,
        CompressLevel::Strong => Some(1080),
        CompressLevel::Max => Some(720),
    }
}

/// Size of the video after `max_height` (never larger, aspect ratio kept).
fn video_size(media: &MediaInfo, max_height: Option<u32>) -> Option<(u32, u32)> {
    let (w, h) = media
        .primary_video()
        .and_then(|v| v.width.zip(v.height))
        .filter(|(w, h)| *w > 0 && *h > 0)?;
    Some(match max_height {
        Some(max) if max < h => ((w as u64 * max as u64 / h as u64) as u32, max),
        _ => (w, h),
    })
}

fn video_fps(media: &MediaInfo, cap: Option<f64>) -> f64 {
    let fps = media.primary_video().and_then(|v| v.fps).unwrap_or(30.0);
    cap.map_or(fps, |c| fps.min(c)).clamp(1.0, 60.0)
}

/// Bit rate of the source video, if known (the whole file's minus its audio).
fn source_video_kbps(media: &MediaInfo) -> Option<f64> {
    let video = media.primary_video()?;
    let bps = video.bit_rate.or_else(|| {
        let audio: u64 = media.audio.iter().filter_map(|a| a.bit_rate).sum();
        media.bit_rate.map(|b| b.saturating_sub(audio))
    })?;
    (bps > 0).then(|| bps as f64 / 1000.0)
}

fn video_for_level(
    media: &MediaInfo,
    codec: VideoCodec,
    audio: Option<AudioCodec>,
    level: CompressLevel,
) -> AdvancedOptions {
    let max_height = level_height(level);
    let fps_cap = (level == CompressLevel::Max).then_some(30.0);
    let source_fps = media.primary_video().and_then(|v| v.fps);
    let fps = fps_cap.filter(|cap| source_fps.is_some_and(|f| f > cap + 0.5));
    let kbps = bits_per_pixel(level).and_then(|bpp| {
        let (w, h) = video_size(media, max_height)?;
        let mut kbps =
            w as f64 * h as f64 * video_fps(media, fps_cap) * bpp / efficiency(codec) / 1000.0;
        if let (Some(source), Some((sw, sh))) = (source_video_kbps(media), video_size(media, None))
        {
            // Never more than the source needs: it would only grow the file.
            let area = (w as f64 * h as f64) / (sw as f64 * sh as f64);
            kbps = kbps.min(source * area * 0.8);
        }
        Some((kbps as u32).max(100))
    });
    let mut advanced = AdvancedOptions {
        max_height: max_height.filter(|max| video_size(media, None).is_some_and(|(_, h)| h > *max)),
        fps,
        video_bitrate_kbps: kbps,
        ..AdvancedOptions::default()
    };
    if let Some(audio) = audio {
        let channels = media.primary_audio().and_then(|a| a.channels).unwrap_or(2);
        let audio_options = audio_for_level(audio, level, channels);
        advanced.audio_bitrate_kbps = audio_options.audio_bitrate_kbps;
        advanced.channels = audio_options.channels;
    }
    advanced
}

/// Video heights tried when the limit leaves few bits per frame.
const HEIGHTS: [u32; 9] = [2160, 1440, 1080, 720, 540, 480, 360, 240, 144];

/// Settings for a video of at most `bytes`: the bit rate the limit allows, and
/// a smaller picture when that rate is too low for the original one.
fn video_for_size(
    media: &MediaInfo,
    codec: VideoCodec,
    audio: Option<AudioCodec>,
    level: CompressLevel,
    bytes: u64,
    secs: f64,
) -> AdvancedOptions {
    let mut advanced = video_for_level(media, codec, audio, level);
    // Room for the container and the encoders' overshoot.
    let total = bytes as f64 * 8.0 / 1000.0 / secs * 0.95;
    let has_audio = audio.is_some() && media.primary_audio().is_some();
    let audio_kbps = if has_audio {
        let codec = audio.expect("checked above");
        let level_kbps = advanced.audio_bitrate_kbps.unwrap_or(96) as f64;
        // The sound gets at most a fifth of a tight budget.
        let kbps = level_kbps
            .min(total / 5.0)
            .max(min_audio_kbps(codec) as f64);
        let kbps = snap_audio_kbps(codec, kbps as u32);
        advanced.audio_bitrate_kbps = Some(kbps);
        if kbps < 64 {
            advanced.channels = media
                .primary_audio()
                .and_then(|a| a.channels)
                .filter(|c| *c > 1)
                .map(|_| ChannelLayout::Mono);
        }
        kbps as f64
    } else {
        0.0
    };
    let video_kbps = (total - audio_kbps).max(30.0);
    let level_kbps = advanced.video_bitrate_kbps.map(f64::from);
    let kbps = level_kbps.map_or(video_kbps, |k| k.min(video_kbps));
    advanced.video_bitrate_kbps = Some(kbps as u32);

    // Enough pixels for about 0.04 bits each (H.264), never more than the level allows.
    let fps = video_fps(media, Some(30.0));
    let budget = kbps * 1000.0 * efficiency(codec) / (fps * 0.04);
    if let Some((w, h)) = video_size(media, None) {
        let aspect = w as f64 / h as f64;
        let level_max = level_height(level).unwrap_or(h);
        let height = HEIGHTS
            .into_iter()
            .filter(|&height| height <= h.min(level_max))
            .find(|&height| height as f64 * height as f64 * aspect <= budget)
            .unwrap_or(HEIGHTS[HEIGHTS.len() - 1].min(h));
        advanced.max_height = (height < h).then_some(height);
    }
    if media
        .primary_video()
        .and_then(|v| v.fps)
        .is_some_and(|f| f > 30.5)
    {
        advanced.fps = Some(30.0);
    }
    advanced
}

/// Animations (GIF, animated WebP, APNG): narrower and fewer pictures per
/// second, from the chosen level to the strongest.
fn animation_ladder(media: &MediaInfo, level: CompressLevel) -> Vec<AdvancedOptions> {
    const STEPS: [(u32, f64); 6] = [
        (640, 15.0),
        (480, 12.0),
        (320, 10.0),
        (240, 8.0),
        (160, 6.0),
        (120, 5.0),
    ];
    let start = match level {
        CompressLevel::Light => 0,
        CompressLevel::Strong => 1,
        CompressLevel::Max => 2,
    };
    let size = video_size(media, None);
    let source_fps = media.primary_video().and_then(|v| v.fps);
    STEPS[start..]
        .iter()
        .map(|&(width, fps)| AdvancedOptions {
            // The planner scales animations to a height: the one giving `width`.
            max_height: size.map(|(w, h)| {
                if w > width {
                    ((h as u64 * width as u64 / w as u64) as u32).max(1)
                } else {
                    h
                }
            }),
            fps: Some(source_fps.map_or(fps, |f| f.min(fps))),
            ..AdvancedOptions::default()
        })
        .collect()
}

// --- Audio -------------------------------------------------------------------------

/// Bit rate of the lossy audio encoders at each level, for stereo (the
/// strongest level is mono).
fn audio_level_kbps(codec: AudioCodec, level: CompressLevel) -> Option<u32> {
    let (light, strong, max) = match codec {
        AudioCodec::Mp3 => (128, 96, 64),
        AudioCodec::Aac => (128, 96, 48),
        AudioCodec::Opus => (64, 48, 24),
        AudioCodec::Vorbis => (112, 80, 48),
        AudioCodec::Wma => (128, 96, 64),
        AudioCodec::Ac3 => (192, 128, 96),
        AudioCodec::Eac3 => (128, 96, 64),
        AudioCodec::Mp2 => (160, 128, 96),
        AudioCodec::Pcm | AudioCodec::Flac | AudioCodec::Alac => return None,
    };
    Some(match level {
        CompressLevel::Light => light,
        CompressLevel::Strong => strong,
        CompressLevel::Max => max,
    })
}

/// Lowest bit rate worth asking of each encoder.
fn min_audio_kbps(codec: AudioCodec) -> u32 {
    match codec {
        AudioCodec::Opus => 6,
        AudioCodec::Mp3 => 8,
        AudioCodec::Aac => 16,
        AudioCodec::Wma => 24,
        _ => 32,
    }
}

/// Bit rates of encoders that only accept their standard table, rounded down.
fn snap_audio_kbps(codec: AudioCodec, kbps: u32) -> u32 {
    const AC3: [u32; 19] = [
        32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 576, 640,
    ];
    const MP2: [u32; 14] = [
        32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
    ];
    let table: &[u32] = match codec {
        AudioCodec::Ac3 => &AC3,
        AudioCodec::Mp2 => &MP2,
        _ => return kbps.max(min_audio_kbps(codec)),
    };
    table
        .iter()
        .copied()
        .filter(|&k| k <= kbps)
        .max()
        .unwrap_or(table[0])
}

fn mono(channels: u32) -> Option<ChannelLayout> {
    (channels > 1).then_some(ChannelLayout::Mono)
}

fn audio_for_level(codec: AudioCodec, level: CompressLevel, channels: u32) -> AdvancedOptions {
    let Some(kbps) = audio_level_kbps(codec, level) else {
        return AdvancedOptions::default();
    };
    let strongest = level == CompressLevel::Max;
    AdvancedOptions {
        audio_bitrate_kbps: Some(snap_audio_kbps(codec, kbps)),
        channels: if strongest { mono(channels) } else { None },
        ..AdvancedOptions::default()
    }
}

/// Lossy audio of at most `bytes`: the bit rate the limit allows (mono below
/// 64 kbit/s), never more than the level's.
fn audio_for_size(
    codec: AudioCodec,
    level: CompressLevel,
    channels: u32,
    bytes: u64,
    secs: f64,
) -> AdvancedOptions {
    let mut advanced = audio_for_level(codec, level, channels);
    let allowed = (bytes as f64 * 8.0 / 1000.0 / secs * 0.96) as u32;
    let level_kbps = advanced.audio_bitrate_kbps.unwrap_or(allowed);
    let kbps = snap_audio_kbps(codec, allowed.min(level_kbps));
    advanced.audio_bitrate_kbps = Some(kbps);
    if kbps < 64 {
        advanced.channels = mono(channels);
    }
    advanced
}

/// Lossless audio (WAV, FLAC, AIFF, ALAC) stays lossless: 16-bit, then a
/// lower sample rate and mono, from the chosen level to the strongest.
fn lossless_audio_ladder(
    level: CompressLevel,
    rate: Option<u32>,
    channels: u32,
) -> Vec<AdvancedOptions> {
    let steps: [(Option<u32>, bool); 6] = [
        (None, false),
        (Some(44_100), false),
        (Some(22_050), true),
        (Some(16_000), true),
        (Some(11_025), true),
        (Some(8_000), true),
    ];
    let start = match level {
        CompressLevel::Light => 0,
        CompressLevel::Strong => 1,
        CompressLevel::Max => 2,
    };
    let mut ladder: Vec<AdvancedOptions> = Vec::new();
    for &(max_rate, to_mono) in &steps[start..] {
        let sample_rate = max_rate.filter(|max| rate.is_none_or(|r| r > *max));
        let options = AdvancedOptions {
            sample_rate,
            channels: if to_mono { mono(channels) } else { None },
            ..AdvancedOptions::default()
        };
        if ladder.last() != Some(&options) {
            ladder.push(options);
        }
    }
    ladder
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::{AudioStream, PictureInfo, VideoStream};

    fn media(kind: MediaKind, video: Vec<VideoStream>, audio: Vec<AudioStream>) -> MediaInfo {
        MediaInfo {
            container: String::new(),
            kind,
            duration_secs: None,
            size_bytes: Some(4_000_000),
            bit_rate: None,
            audio,
            video,
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

    fn stream(
        codec: &str,
        width: u32,
        height: u32,
        fps: Option<f64>,
        pix_fmt: &str,
    ) -> VideoStream {
        VideoStream {
            index: 0,
            codec: codec.into(),
            width: Some(width),
            height: Some(height),
            fps,
            bit_rate: None,
            pix_fmt: Some(pix_fmt.into()),
        }
    }

    fn video(width: u32, height: u32, fps: f64, secs: f64, codec: &str) -> MediaInfo {
        let sound = AudioStream {
            index: 1,
            codec: "aac".into(),
            sample_rate: Some(48_000),
            channels: Some(2),
            bit_rate: Some(192_000),
            bit_depth: None,
        };
        let mut picture = stream(codec, width, height, Some(fps), "yuv420p");
        picture.bit_rate = Some(15_000_000);
        MediaInfo {
            duration_secs: Some(secs),
            size_bytes: Some(200_000_000),
            bit_rate: Some(16_000_000),
            ..media(MediaKind::VideoWithAudio, vec![picture], vec![sound])
        }
    }

    fn photo(width: u32, height: u32, codec: &str, alpha: bool) -> MediaInfo {
        let pix_fmt = if alpha { "rgba" } else { "yuvj420p" };
        MediaInfo {
            picture: Some(PictureInfo {
                alpha,
                ..PictureInfo::default()
            }),
            ..media(
                MediaKind::Image,
                vec![stream(codec, width, height, None, pix_fmt)],
                Vec::new(),
            )
        }
    }

    fn options(level: CompressLevel, max_bytes: Option<u64>) -> CompressOptions {
        CompressOptions {
            level,
            max_bytes,
            ..CompressOptions::default()
        }
    }

    #[test]
    fn files_are_compressed_in_their_own_format() {
        let jpg = photo(4032, 3024, "mjpeg", false);
        assert_eq!(
            compress_format(&jpg, Path::new("a.jpg")),
            Some(TargetFormat::Jpg)
        );
        let png = photo(800, 600, "png", true);
        assert_eq!(
            compress_format(&png, Path::new("a.PNG")),
            Some(TargetFormat::Png)
        );
        assert_eq!(
            compress_format(&photo(800, 600, "bmp", false), Path::new("a.bmp")),
            None
        );
        assert_eq!(
            compress_format(&photo(800, 600, "hevc", false), Path::new("a.heic")),
            None
        );
        let mp4 = video(1920, 1080, 30.0, 60.0, "h264");
        assert_eq!(
            compress_format(&mp4, Path::new("clip.mp4")),
            Some(TargetFormat::Mp4)
        );
        let pdf = media(MediaKind::Document, Vec::new(), Vec::new());
        assert_eq!(
            compress_format(&pdf, Path::new("a.pdf")),
            Some(TargetFormat::Pdf)
        );
    }

    #[test]
    fn photos_get_lower_qualities_then_smaller() {
        let steps = picture_ladder(ImageCodec::Jpeg, CompressLevel::Strong, 4032, false);
        assert_eq!(
            steps[0],
            PictureStep {
                quality: Some(65),
                side: Some(2560),
                palette: false
            }
        );
        let qualities: Vec<u8> = steps
            .iter()
            .filter(|s| s.side == Some(2560))
            .filter_map(|s| s.quality)
            .collect();
        assert_eq!(qualities, [65, 55, 45, 35, 25]);
        let sides: Vec<u32> = steps
            .iter()
            .filter_map(|s| s.side)
            .filter(|s| *s < 2560)
            .collect();
        assert_eq!(sides, [1920, 1440, 1080, 810, 607, 455, 341]);
        // A picture already smaller than the level's side keeps its size first.
        let small = picture_ladder(ImageCodec::Webp, CompressLevel::Max, 1000, false);
        assert_eq!(small[0].side, None);
        assert_eq!(small[0].quality, Some(40));
    }

    #[test]
    fn png_keeps_every_pixel_then_uses_a_palette() {
        let steps = picture_ladder(ImageCodec::Png, CompressLevel::Light, 1200, true);
        assert!(!steps[0].palette && steps[0].quality.is_none());
        assert!(steps[1].palette);
        // Transparent pictures keep smooth edges at the strong level.
        assert!(!picture_ladder(ImageCodec::Png, CompressLevel::Strong, 1200, true)[0].palette);
        assert!(picture_ladder(ImageCodec::Png, CompressLevel::Strong, 1200, false)[0].palette);
        assert!(picture_ladder(ImageCodec::Png, CompressLevel::Max, 1200, true)[0].palette);
    }

    #[test]
    fn png_with_fewer_colors_goes_through_a_palette() {
        let media = photo(800, 600, "png", false);
        let search = Search::new(
            &media,
            &options(CompressLevel::Strong, None),
            TargetFormat::Png,
            false,
        );
        let attempt = search.first();
        assert!(attempt.palette);
        let plan = attempt.plan(&media, Path::new("a.png")).unwrap();
        assert_eq!(plan.image.as_ref().unwrap().quality, ImageQuality::Palette);
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
        let args = crate::command::build_from_plan(&plan, Path::new("a.png"), Path::new("b.png"))
            .args_lossy();
        let graph = &args[args.iter().position(|a| a == "-filter_complex").unwrap() + 1];
        assert!(
            graph.contains("palettegen") && graph.contains("paletteuse"),
            "{graph}"
        );
    }

    #[test]
    fn video_levels_lower_the_resolution_and_the_bit_rate() {
        let media = video(3840, 2160, 60.0, 60.0, "h264");
        let light = video_for_level(
            &media,
            VideoCodec::H264,
            Some(AudioCodec::Aac),
            CompressLevel::Light,
        );
        assert_eq!((light.max_height, light.video_bitrate_kbps), (None, None));
        assert_eq!(light.audio_bitrate_kbps, Some(128));
        let max = video_for_level(
            &media,
            VideoCodec::H264,
            Some(AudioCodec::Aac),
            CompressLevel::Max,
        );
        assert_eq!(max.max_height, Some(720));
        assert_eq!(max.fps, Some(30.0));
        // 1280×720 × 30 fps × 0.022 bits.
        assert_eq!(max.video_bitrate_kbps, Some(608));
        assert_eq!(max.channels, Some(ChannelLayout::Mono));
        // VP9 needs fewer bits for the same picture.
        let vp9 = video_for_level(
            &media,
            VideoCodec::Vp9,
            Some(AudioCodec::Opus),
            CompressLevel::Max,
        );
        assert!(vp9.video_bitrate_kbps.unwrap() < 608);
    }

    #[test]
    fn a_size_limit_sets_the_bit_rate_and_the_picture_size() {
        let media = video(1920, 1080, 30.0, 120.0, "h264");
        // 10 MB for two minutes: about 633 kbit/s in all.
        let adv = video_for_size(
            &media,
            VideoCodec::H264,
            Some(AudioCodec::Aac),
            CompressLevel::Light,
            10_000_000,
            120.0,
        );
        let (video, audio) = (
            adv.video_bitrate_kbps.unwrap(),
            adv.audio_bitrate_kbps.unwrap(),
        );
        assert!(
            video + audio <= 633 && video + audio > 550,
            "{video} + {audio}"
        );
        assert_eq!(adv.max_height, Some(480));
        let total = (video + audio) as f64 * 1000.0 / 8.0 * 120.0;
        assert!(total < 10_000_000.0);
    }

    #[test]
    fn rates_are_corrected_when_the_file_is_too_large() {
        let media = video(1280, 720, 30.0, 100.0, "h264");
        let mut search = Search::new(
            &media,
            &options(CompressLevel::Light, Some(5_000_000)),
            TargetFormat::Mp4,
            false,
        );
        let first = search.first();
        let kbps = first.request.advanced.video_bitrate_kbps.unwrap();
        assert!(search.fits(5_000_000) && !search.fits(5_000_001));
        assert_eq!(search.next(&first, 4_000_000), None);
        let second = search.next(&first, 6_000_000).unwrap();
        let lower = second.request.advanced.video_bitrate_kbps.unwrap();
        assert!(lower < kbps * 80 / 100, "{kbps} → {lower}");
        let third = search.next(&second, 5_500_000).unwrap();
        assert!(search.next(&third, 5_200_000).is_none());
    }

    #[test]
    fn audio_levels_and_limits() {
        let opus = audio_for_level(AudioCodec::Opus, CompressLevel::Max, 2);
        assert_eq!(
            (opus.audio_bitrate_kbps, opus.channels),
            (Some(24), Some(ChannelLayout::Mono))
        );
        let mp3 = audio_for_size(AudioCodec::Mp3, CompressLevel::Light, 2, 3_000_000, 300.0);
        // 3 MB for five minutes: 76 kbit/s.
        assert_eq!(mp3.audio_bitrate_kbps, Some(76));
        assert_eq!(snap_audio_kbps(AudioCodec::Ac3, 100), 96);
        assert_eq!(snap_audio_kbps(AudioCodec::Mp2, 20), 32);
        let ladder = lossless_audio_ladder(CompressLevel::Strong, Some(96_000), 2);
        assert_eq!(ladder[0].sample_rate, Some(44_100));
        assert_eq!(ladder[1].channels, Some(ChannelLayout::Mono));
        // A 22.05 kHz source: no step repeats the same settings.
        let low = lossless_audio_ladder(CompressLevel::Light, Some(22_050), 1);
        assert_eq!(low.len(), 4);
    }

    #[test]
    fn animations_get_narrower() {
        let media = MediaInfo {
            kind: MediaKind::Video,
            ..video(800, 400, 25.0, 4.0, "gif")
        };
        let ladder = animation_ladder(&media, CompressLevel::Strong);
        assert_eq!(
            (ladder[0].max_height, ladder[0].fps),
            (Some(240), Some(12.0))
        );
        assert_eq!(ladder.len(), 5);
    }
}
