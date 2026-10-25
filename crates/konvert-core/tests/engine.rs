//! Integration tests against the real FFmpeg/FFprobe binaries.
//!
//! Requires `scripts/setup-ffmpeg.*` to have been run (or `KONVERT_FFMPEG_DIR`).
//! Sample media are generated on the fly with FFmpeg's `lavfi` sources inside a
//! temporary directory whose name contains spaces and non-ASCII characters; no
//! media file is ever stored in the repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use konvert_core::{
    Binaries, ConversionJob, ConversionRequest, CoreError, MediaKind, QualityPreset, TargetFormat,
    Workspace, probe, run_conversion,
};
use tokio_util::sync::CancellationToken;

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

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn wav_to_mp3_end_to_end() {
    let bins = engine();
    let tmp = TempDir::new("wav-mp3");
    let input = tmp.0.join("Brano d’esempio è.wav");
    generate(
        &bins,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=5",
            "-ac",
            "2",
            "-metadata",
            "title=Titolo àè",
        ],
        &input,
    );

    let media = probe(&bins, &input).await.unwrap();
    assert_eq!(media.kind, MediaKind::Audio);
    assert!((media.duration_secs.unwrap() - 5.0).abs() < 0.1);

    let out_dir = tmp.0.join("uscita");
    std::fs::create_dir_all(&out_dir).unwrap();
    let ws = Workspace::create(&tmp.0.join("ws")).unwrap();
    let job = ConversionJob {
        id: 7,
        input: input.clone(),
        media,
        request: ConversionRequest {
            target: TargetFormat::Mp3,
            preset: QualityPreset::Balanced,
        },
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
    // Nothing is left in the private temporary directory.
    assert_eq!(files_in(ws.dir()), [".lock"]);

    let converted = probe(&bins, &outcome.output).await.unwrap();
    assert_eq!(converted.container, "mp3");
    assert_eq!(converted.primary_audio().unwrap().codec, "mp3");
    assert!((converted.duration_secs.unwrap() - 5.0).abs() < 0.2);

    // A second conversion never overwrites the first output.
    let second = run_conversion(&bins, &ws, &job, &CancellationToken::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(second.output, out_dir.join("Brano d’esempio è (1).mp3"));
}

#[tokio::test]
async fn cancellation_removes_the_partial_output() {
    let bins = engine();
    let tmp = TempDir::new("cancel");
    // Long but tiny on disk (a pure tone compresses extremely well in FLAC).
    let input = tmp.0.join("lungo.flac");
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
    let out_dir = tmp.0.join("uscita");
    std::fs::create_dir_all(&out_dir).unwrap();
    let ws = Workspace::create(&tmp.0.join("ws")).unwrap();
    let job = ConversionJob {
        id: 1,
        input,
        media,
        request: ConversionRequest {
            target: TargetFormat::Mp3,
            preset: QualityPreset::Max,
        },
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
    let fake = tmp.0.join("non è un mp3.mp3");
    std::fs::write(&fake, b"this is definitely not audio data, just text").unwrap();
    let err = probe(&bins, &fake).await.unwrap_err();
    assert!(
        matches!(err, CoreError::CorruptInput { .. } | CoreError::NotMedia),
        "{err:?}"
    );
    assert!(err.details().is_none() || !err.to_string().contains("Invalid data"));
}
