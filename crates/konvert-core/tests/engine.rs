//! Integration tests against the real FFmpeg/FFprobe binaries.
//!
//! Requires `scripts/setup-ffmpeg.*` to have been run (or `KONVERT_FFMPEG_DIR`).
//! Sample media are generated on the fly with FFmpeg's `lavfi` sources inside a
//! temporary directory whose name contains spaces and non-ASCII characters; no
//! media file is ever stored in the repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use konvert_core::{
    AdvancedOptions, Binaries, ChannelLayout, ConversionJob, ConversionOutcome, ConversionRequest,
    CoreError, MediaInfo, MediaKind, QualityPreset, TargetFormat, Workspace, plan_conversion,
    probe, run_conversion,
};
use tokio_util::sync::CancellationToken;

// --- Helpers -------------------------------------------------------------------

fn engine() -> Binaries {
    let dir = std::env::var_os("KONVERT_FFMPEG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/binaries/ffmpeg")
        });
    Binaries::from_dir(&dir).unwrap_or_else(|_| {
        panic!("FFmpeg binaries not found: run scripts/setup-ffmpeg.ps1 (or .sh) first")
    })
}

/// A fresh temporary directory with spaces and Unicode in its name.
struct TempDir(PathBuf);

impl TempDir {
    fn new(test: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "konvert test {test} – àèìòù 🎧 {}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Generates a sample file with FFmpeg (test fixture only, `lavfi` input).
fn generate(bins: &Binaries, args: &[&str], output: &Path) {
    let status = Command::new(&bins.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .arg(output)
        .status()
        .expect("ffmpeg runs");
    assert!(status.success(), "failed to generate {}", output.display());
}

/// 3 s stereo 44.1 kHz 16-bit WAV with a title tag.
fn sample_wav(bins: &Binaries, tmp: &TempDir) -> PathBuf {
    let path = tmp.path("Brano d’esempio è.wav");
    generate(
        bins,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=3",
            "-ac",
            "2",
            "-metadata",
            "title=Titolo àè",
        ],
        &path,
    );
    path
}

/// 3 s 320×240 25 fps H.264 + AAC MP4.
fn sample_mp4(bins: &Binaries, tmp: &TempDir) -> PathBuf {
    let path = tmp.path("Clip di prova.mp4");
    generate(
        bins,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=3",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=330:duration=3",
            "-c:v",
            "libopenh264",
            "-b:v",
            "400k",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "96k",
            "-ac",
            "2",
            "-shortest",
        ],
        &path,
    );
    path
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Probes, converts into `tmp/out`, probes the result.
async fn convert(
    bins: &Binaries,
    tmp: &TempDir,
    input: &Path,
    request: ConversionRequest,
) -> (ConversionOutcome, MediaInfo) {
    let media = probe(bins, input).await.unwrap();
    let out_dir = tmp.path("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let ws = Workspace::create(&tmp.path("ws")).unwrap();
    let job = ConversionJob {
        id: 1,
        input: input.to_path_buf(),
        media,
        request: request.clone(),
        output_dir: out_dir,
    };
    let outcome = run_conversion(bins, &ws, &job, &CancellationToken::new(), |_| {})
        .await
        .unwrap_or_else(|e| panic!("{:?} failed: {e} {:?}", request.target, e.details()));
    assert_eq!(files_in(ws.dir()), [".lock"], "temporary files left behind");
    let converted = probe(bins, &outcome.output).await.unwrap();
    (outcome, converted)
}

fn assert_duration(info: &MediaInfo, expected: f64) {
    let d = info.duration_secs.expect("duration");
    assert!(
        (d - expected).abs() < 0.3,
        "duration {d}, expected {expected}"
    );
}

fn req(target: TargetFormat, preset: QualityPreset) -> ConversionRequest {
    ConversionRequest::new(target, preset)
}

// --- Conversions -----------------------------------------------------------------

#[tokio::test]
async fn wav_to_mp3_end_to_end() {
    let bins = engine();
    let tmp = TempDir::new("wav-mp3");
    let input = sample_wav(&bins, &tmp);
    let media = probe(&bins, &input).await.unwrap();
    assert_eq!(media.kind, MediaKind::Audio);

    let out_dir = tmp.path("uscita");
    std::fs::create_dir_all(&out_dir).unwrap();
    let ws = Workspace::create(&tmp.path("ws")).unwrap();
    let job = ConversionJob {
        id: 7,
        input: input.clone(),
        media,
        request: req(TargetFormat::Mp3, QualityPreset::Balanced),
        output_dir: out_dir.clone(),
    };

    let mut fractions = Vec::new();
    let outcome = run_conversion(&bins, &ws, &job, &CancellationToken::new(), |p| {
        fractions.push(p.fraction)
    })
    .await
    .unwrap();

    assert_eq!(outcome.output, out_dir.join("Brano d’esempio è.mp3"));
    assert!(outcome.size_bytes > 0);
    assert_eq!(fractions.last().copied().flatten(), Some(1.0));
    assert_eq!(files_in(ws.dir()), [".lock"]);

    let converted = probe(&bins, &outcome.output).await.unwrap();
    assert_eq!(converted.container, "mp3");
    assert_eq!(converted.primary_audio().unwrap().codec, "mp3");
    assert_duration(&converted, 3.0);

    // A second conversion never overwrites the first output.
    let second = run_conversion(&bins, &ws, &job, &CancellationToken::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(second.output, out_dir.join("Brano d’esempio è (1).mp3"));
}

#[tokio::test]
async fn mp4_to_mp3_extracts_the_soundtrack() {
    let bins = engine();
    let tmp = TempDir::new("mp4-mp3");
    let input = sample_mp4(&bins, &tmp);
    let (outcome, out) = convert(
        &bins,
        &tmp,
        &input,
        req(TargetFormat::Mp3, QualityPreset::Balanced),
    )
    .await;
    assert!(outcome.output.ends_with("Clip di prova.mp3"));
    assert_eq!(out.kind, MediaKind::Audio);
    assert_eq!(out.primary_audio().unwrap().codec, "mp3");
    assert!(out.video.is_empty());
    assert_duration(&out, 3.0);
}

#[tokio::test]
async fn mp4_to_webm_reencodes_to_vp9_and_opus() {
    let bins = engine();
    let tmp = TempDir::new("mp4-webm");
    let input = sample_mp4(&bins, &tmp);
    let (_, out) = convert(
        &bins,
        &tmp,
        &input,
        req(TargetFormat::Webm, QualityPreset::Balanced),
    )
    .await;
    assert_eq!(out.container, "matroska,webm");
    assert_eq!(out.kind, MediaKind::VideoWithAudio);
    let video = out.primary_video().unwrap();
    assert_eq!(video.codec, "vp9");
    assert_eq!((video.width, video.height), (Some(320), Some(240)));
    assert_eq!(out.primary_audio().unwrap().codec, "opus");
    assert_duration(&out, 3.0);
}

#[tokio::test]
async fn every_audio_format_from_wav() {
    let bins = engine();
    let tmp = TempDir::new("audio-matrix");
    let input = sample_wav(&bins, &tmp);
    let cases = [
        (TargetFormat::Mp3, "mp3", "mp3"),
        (TargetFormat::M4a, "mov,mp4,m4a,3gp,3g2,mj2", "aac"),
        (TargetFormat::Flac, "flac", "flac"),
        (TargetFormat::Ogg, "ogg", "vorbis"),
        (TargetFormat::Opus, "ogg", "opus"),
        (TargetFormat::Wma, "asf", "wmav2"),
        (TargetFormat::Aiff, "aiff", "pcm_s16be"),
    ];
    for (target, container, codec) in cases {
        for preset in [QualityPreset::Max, QualityPreset::Small] {
            let (outcome, out) = convert(&bins, &tmp, &input, req(target, preset)).await;
            assert_eq!(out.container, container, "{target:?} {preset:?}");
            assert_eq!(
                out.primary_audio().unwrap().codec,
                codec,
                "{target:?} {preset:?}"
            );
            assert_duration(&out, 3.0);
            std::fs::remove_file(&outcome.output).unwrap();
        }
    }
}

#[tokio::test]
async fn every_video_format_from_mp4() {
    let bins = engine();
    let tmp = TempDir::new("video-matrix");
    let input = sample_mp4(&bins, &tmp);
    // (target, preset, container, video codec, audio codec)
    let cases = [
        // Compatible codecs: copied without re-encoding.
        (
            TargetFormat::Mkv,
            QualityPreset::Balanced,
            "matroska,webm",
            "h264",
            Some("aac"),
        ),
        (
            TargetFormat::Mov,
            QualityPreset::Max,
            "mov,mp4,m4a,3gp,3g2,mj2",
            "h264",
            Some("aac"),
        ),
        // Same format: re-encoded with OpenH264.
        (
            TargetFormat::Mp4,
            QualityPreset::Small,
            "mov,mp4,m4a,3gp,3g2,mj2",
            "h264",
            Some("aac"),
        ),
        (
            TargetFormat::Avi,
            QualityPreset::Balanced,
            "avi",
            "mpeg4",
            Some("mp3"),
        ),
        (TargetFormat::Gif, QualityPreset::Small, "gif", "gif", None),
    ];
    for (target, preset, container, vcodec, acodec) in cases {
        let (outcome, out) = convert(&bins, &tmp, &input, req(target, preset)).await;
        assert_eq!(out.container, container, "{target:?}");
        assert_eq!(out.primary_video().unwrap().codec, vcodec, "{target:?}");
        assert_eq!(
            out.primary_audio().map(|a| a.codec.as_str()),
            acodec,
            "{target:?}"
        );
        std::fs::remove_file(&outcome.output).unwrap();
    }
}

#[tokio::test]
async fn reencoded_mp4_size_is_close_to_the_estimate() {
    let bins = engine();
    let tmp = TempDir::new("mp4-estimate");
    let input = sample_mp4(&bins, &tmp);
    let media = probe(&bins, &input).await.unwrap();
    let request = req(TargetFormat::Mov, QualityPreset::Small);
    let estimate = plan_conversion(&media, &request, &input)
        .unwrap()
        .estimate
        .unwrap();
    let (outcome, _) = convert(&bins, &tmp, &input, request).await;
    let ratio = outcome.size_bytes as f64 / estimate.bytes as f64;
    assert!(
        (0.5..1.5).contains(&ratio),
        "actual {} vs estimate {}",
        outcome.size_bytes,
        estimate.bytes
    );
}

#[tokio::test]
async fn cover_art_and_tags_are_preserved() {
    let bins = engine();
    let tmp = TempDir::new("cover");
    let input = tmp.path("Con copertina.mp3");
    generate(
        &bins,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=duration=2",
            "-f",
            "lavfi",
            "-i",
            "color=c=orange:size=64x64:duration=1",
            "-map",
            "0:a",
            "-map",
            "1:v",
            "-frames:v",
            "1",
            "-c:a",
            "libmp3lame",
            "-c:v",
            "mjpeg",
            "-disposition:v",
            "attached_pic",
            "-metadata",
            "artist=Artista è",
            "-id3v2_version",
            "3",
        ],
        &input,
    );
    let media = probe(&bins, &input).await.unwrap();
    assert_eq!(media.kind, MediaKind::Audio);
    assert!(media.cover_art.is_some());

    for target in [TargetFormat::Flac, TargetFormat::M4a, TargetFormat::Mp3] {
        let (outcome, out) =
            convert(&bins, &tmp, &input, req(target, QualityPreset::Balanced)).await;
        assert!(out.cover_art.is_some(), "{target:?} lost the cover");
        assert_eq!(out.kind, MediaKind::Audio, "{target:?}");
        let tags = Command::new(&bins.ffprobe)
            .args([
                "-v",
                "error",
                "-show_entries",
                "format_tags=artist",
                "-of",
                "default=nw=1:nk=1",
            ])
            .arg(&outcome.output)
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&tags.stdout).trim(),
            "Artista è",
            "{target:?}"
        );
    }
}

#[tokio::test]
async fn advanced_options_are_applied() {
    let bins = engine();
    let tmp = TempDir::new("advanced");
    let input = sample_mp4(&bins, &tmp);
    let request = ConversionRequest {
        target: TargetFormat::Mkv,
        preset: QualityPreset::Balanced,
        advanced: AdvancedOptions {
            max_height: Some(120),
            fps: Some(15.0),
            channels: Some(ChannelLayout::Mono),
            sample_rate: Some(32000),
            ..Default::default()
        },
    };
    let (_, out) = convert(&bins, &tmp, &input, request).await;
    let video = out.primary_video().unwrap();
    assert_eq!((video.width, video.height), (Some(160), Some(120)));
    assert!((video.fps.unwrap() - 15.0).abs() < 0.01, "{:?}", video.fps);
    let audio = out.primary_audio().unwrap();
    assert_eq!((audio.channels, audio.sample_rate), (Some(1), Some(32000)));
}

#[tokio::test]
async fn high_resolution_audio_keeps_24_bit_and_pcm_estimate_is_exact() {
    let bins = engine();
    let tmp = TempDir::new("hires");
    let input = tmp.path("hires.wav");
    generate(
        &bins,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=duration=2:sample_rate=96000",
            "-ac",
            "2",
            "-c:a",
            "pcm_s24le",
        ],
        &input,
    );
    let (_, flac) = convert(
        &bins,
        &tmp,
        &input,
        req(TargetFormat::Flac, QualityPreset::Max),
    )
    .await;
    let a = flac.primary_audio().unwrap();
    assert_eq!((a.bit_depth, a.sample_rate), (Some(24), Some(96000)));

    let (_, small) = convert(
        &bins,
        &tmp,
        &input,
        req(TargetFormat::Flac, QualityPreset::Small),
    )
    .await;
    assert_eq!(small.primary_audio().unwrap().bit_depth, Some(16));

    let media = probe(&bins, &input).await.unwrap();
    let request = req(TargetFormat::Aiff, QualityPreset::Max);
    let estimate = plan_conversion(&media, &request, &input)
        .unwrap()
        .estimate
        .unwrap();
    let (outcome, aiff) = convert(&bins, &tmp, &input, request).await;
    assert_eq!(aiff.primary_audio().unwrap().codec, "pcm_s24be");
    let ratio = outcome.size_bytes as f64 / estimate.bytes as f64;
    assert!(
        (0.97..1.03).contains(&ratio),
        "actual {} vs estimate {}",
        outcome.size_bytes,
        estimate.bytes
    );
}

// --- Robustness --------------------------------------------------------------------

#[tokio::test]
async fn cancellation_removes_the_partial_output() {
    let bins = engine();
    let tmp = TempDir::new("cancel");
    // Long but tiny on disk (a pure tone compresses extremely well in FLAC).
    let input = tmp.path("lungo.flac");
    generate(
        &bins,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=300:duration=1800:sample_rate=22050",
        ],
        &input,
    );
    let media = probe(&bins, &input).await.unwrap();
    let out_dir = tmp.path("uscita");
    std::fs::create_dir_all(&out_dir).unwrap();
    let ws = Workspace::create(&tmp.path("ws")).unwrap();
    let job = ConversionJob {
        id: 1,
        input,
        media,
        request: req(TargetFormat::Mp3, QualityPreset::Max),
        output_dir: out_dir.clone(),
    };

    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let result = run_conversion(&bins, &ws, &job, &cancel, move |p| {
        if p.fraction.is_some_and(|f| f > 0.0 && f < 1.0) {
            trigger.cancel();
        }
    })
    .await;

    assert!(matches!(result, Err(CoreError::Cancelled)), "{result:?}");
    assert!(files_in(&out_dir).is_empty(), "no output must be created");
    assert_eq!(
        files_in(ws.dir()),
        [".lock"],
        "partial file must be deleted"
    );
}

#[tokio::test]
async fn network_urls_are_never_opened() {
    let bins = engine();
    for url in [
        "http://127.0.0.1:9/a.mp3",
        "https://example.com/a.wav",
        "rtmp://127.0.0.1/x",
    ] {
        let err = probe(&bins, Path::new(url)).await.unwrap_err();
        assert_eq!(err.code(), "input_not_found", "{url}");
    }
}

#[tokio::test]
async fn damaged_files_are_reported_clearly() {
    let bins = engine();
    let tmp = TempDir::new("corrupt");
    let fake = tmp.path("non è un mp3.mp3");
    std::fs::write(&fake, b"this is definitely not audio data, just text").unwrap();
    let err = probe(&bins, &fake).await.unwrap_err();
    assert!(
        matches!(err, CoreError::CorruptInput { .. } | CoreError::NotMedia),
        "{err:?}"
    );
}
