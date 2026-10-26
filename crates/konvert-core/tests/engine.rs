//! Integration tests against the real FFmpeg/FFprobe binaries.
//!
//! Requires `scripts/setup-ffmpeg.*` to have been run (or `KONVERT_FFMPEG_DIR`).
//! Sample media are generated on the fly with FFmpeg's `lavfi` sources inside a
//! temporary directory whose name contains spaces and non-ASCII characters; no
//! media file is ever stored in the repository.

mod fixtures;

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
    // Output captured: some encoders (kvazaar) print their banner anyway.
    let result = Command::new(&bins.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .arg(output)
        .output()
        .expect("ffmpeg runs");
    assert!(
        result.status.success(),
        "failed to generate {}: {}",
        output.display(),
        String::from_utf8_lossy(&result.stderr)
    );
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

// --- Pictures ----------------------------------------------------------------------

/// A 64×48 test picture written by FFmpeg with the given encoder options.
fn sample_picture(bins: &Binaries, tmp: &TempDir, name: &str, args: &[&str]) -> PathBuf {
    let path = tmp.path(name);
    let mut all = vec!["-f", "lavfi", "-i", "testsrc2=size=64x48", "-frames:v", "1"];
    all.extend(args);
    generate(bins, &all, &path);
    path
}

/// A 64×48 JPEG photo, optionally with EXIF data and an orientation.
fn sample_jpeg(bins: &Binaries, tmp: &TempDir, name: &str, orientation: Option<u16>) -> PathBuf {
    let path = sample_picture(bins, tmp, name, &["-c:v", "mjpeg", "-f", "image2pipe"]);
    if let Some(orientation) = orientation {
        let jpeg = std::fs::read(&path).unwrap();
        std::fs::write(&path, fixtures::jpeg_with_exif(&jpeg, orientation)).unwrap();
    }
    path
}

/// 64×48 PNG: transparent left half, opaque blue right half.
fn sample_transparent_png(bins: &Binaries, tmp: &TempDir) -> PathBuf {
    let path = tmp.path("Logo trasparente.png");
    generate(
        bins,
        &[
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:size=64x48,format=rgba,geq=r='r(X,Y)':g='g(X,Y)':b='b(X,Y)':a='if(lt(X,32),0,255)'",
            "-frames:v",
            "1",
            "-c:v",
            "png",
            "-f",
            "image2pipe",
        ],
        &path,
    );
    path
}

/// HEIC photo: a 2 × 2 grid of 64×48 HEVC tiles (red, green / blue, white).
fn sample_heic(bins: &Binaries, tmp: &TempDir, name: &str, quarter_turns: u8) -> PathBuf {
    let tiles: Vec<Vec<u8>> = ["red", "green", "blue", "white"]
        .iter()
        .map(|color| {
            let tile = tmp.path(&format!("tile-{color}.hevc"));
            generate(
                bins,
                &[
                    "-f",
                    "lavfi",
                    "-i",
                    &format!("color=c={color}:size=64x48"),
                    "-frames:v",
                    "1",
                    "-c:v",
                    "libkvazaar",
                    "-pix_fmt",
                    "yuv420p",
                    "-f",
                    "hevc",
                ],
                &tile,
            );
            let data = std::fs::read(&tile).unwrap();
            std::fs::remove_file(&tile).unwrap();
            data
        })
        .collect();
    let heic = fixtures::heif_grid(&fixtures::HeifGrid {
        tiles: &tiles,
        columns: 2,
        rows: 2,
        tile_width: 64,
        tile_height: 48,
        quarter_turns,
        // Like a phone: EXIF orientation too, which `irot` overrides.
        exif: Some(fixtures::exif_tiff(if quarter_turns == 0 { 1 } else { 6 })),
    });
    let path = tmp.path(name);
    std::fs::write(&path, heic).unwrap();
    path
}

/// RGBA value of one pixel, decoded by FFmpeg.
fn pixel(bins: &Binaries, picture: &Path, x: u32, y: u32) -> [u8; 4] {
    let out = Command::new(&bins.ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(picture)
        // RGBA first: a 1×1 crop of subsampled YUV would have no chroma.
        .args([
            "-frames:v",
            "1",
            "-vf",
            &format!("format=rgba,crop=1:1:{x}:{y}"),
        ])
        .args(["-f", "rawvideo", "-"])
        .output()
        .unwrap();
    out.stdout[..4].try_into().expect("one RGBA pixel")
}

fn size_of(info: &MediaInfo) -> (Option<u32>, Option<u32>) {
    let v = info.primary_video().unwrap();
    (v.width, v.height)
}

fn picture_codec(target: TargetFormat) -> &'static str {
    match target {
        TargetFormat::Jpg => "mjpeg",
        TargetFormat::Png => "png",
        TargetFormat::Webp => "webp",
        TargetFormat::Avif => "av1",
        other => panic!("{other:?} is not a picture format"),
    }
}

#[tokio::test]
async fn every_picture_format_from_every_picture_input() {
    let bins = engine();
    let tmp = TempDir::new("pictures");
    let inputs = [
        (sample_jpeg(&bins, &tmp, "Foto è.jpg", None), (64, 48)),
        (
            sample_picture(
                &bins,
                &tmp,
                "Disegno.png",
                &["-c:v", "png", "-f", "image2pipe"],
            ),
            (64, 48),
        ),
        (
            sample_picture(
                &bins,
                &tmp,
                "Immagine.webp",
                &["-c:v", "libwebp", "-f", "webp"],
            ),
            (64, 48),
        ),
        (
            sample_picture(
                &bins,
                &tmp,
                "Scansione.bmp",
                &["-c:v", "bmp", "-f", "image2pipe"],
            ),
            (64, 48),
        ),
        (
            sample_picture(
                &bins,
                &tmp,
                "Scansione.tiff",
                &["-c:v", "tiff", "-f", "image2pipe"],
            ),
            (64, 48),
        ),
        (
            sample_picture(
                &bins,
                &tmp,
                "Foto.avif",
                &[
                    "-c:v",
                    "libaom-av1",
                    "-still-picture",
                    "1",
                    "-pix_fmt",
                    "yuv420p",
                    "-f",
                    "avif",
                ],
            ),
            (64, 48),
        ),
        (sample_heic(&bins, &tmp, "IMG_0001.HEIC", 0), (128, 96)),
    ];
    for (input, (width, height)) in &inputs {
        let media = probe(&bins, input).await.unwrap();
        assert_eq!(media.kind, MediaKind::Image, "{input:?}");
        assert_eq!(size_of(&media), (Some(*width), Some(*height)), "{input:?}");
        let targets = konvert_core::available_targets(&media, input);
        assert!(targets.len() >= 3, "{input:?}");
        for target in targets {
            let request = req(target.format, QualityPreset::Balanced);
            let (outcome, out) = convert(&bins, &tmp, input, request).await;
            let what = format!("{input:?} → {:?}", target.format);
            assert_eq!(out.kind, MediaKind::Image, "{what}");
            assert_eq!(
                out.primary_video().unwrap().codec,
                picture_codec(target.format),
                "{what}"
            );
            assert_eq!(size_of(&out), (Some(*width), Some(*height)), "{what}");
            assert!(outcome.size_bytes > 0, "{what}");
            std::fs::remove_file(&outcome.output).unwrap();
        }
    }
}

#[tokio::test]
async fn exif_orientation_is_applied_once_and_exif_kept_in_png() {
    let bins = engine();
    let tmp = TempDir::new("orientation");
    // Stored 64×48, displayed 48×64.
    let input = sample_jpeg(&bins, &tmp, "Ritratto.jpg", Some(6));
    let media = probe(&bins, &input).await.unwrap();
    assert_eq!(size_of(&media), (Some(48), Some(64)));
    assert!(media.picture.as_ref().unwrap().exif);

    for target in [
        TargetFormat::Png,
        TargetFormat::Jpg,
        TargetFormat::Webp,
        TargetFormat::Avif,
    ] {
        let (outcome, out) = convert(&bins, &tmp, &input, req(target, QualityPreset::Max)).await;
        // A kept orientation tag would rotate the picture a second time.
        assert_eq!(size_of(&out), (Some(48), Some(64)), "{target:?}");
        let exif_kept = out.picture.as_ref().unwrap().exif;
        assert_eq!(exif_kept, target == TargetFormat::Png, "{target:?}");
        let warned = plan_conversion(&media, &req(target, QualityPreset::Max), &input)
            .unwrap()
            .warnings
            .contains(&konvert_core::ConversionWarning::MetadataDropped);
        assert_eq!(warned, !exif_kept, "{target:?}");
        std::fs::remove_file(&outcome.output).unwrap();
    }
}

#[tokio::test]
async fn transparency_is_kept_or_laid_on_white() {
    let bins = engine();
    let tmp = TempDir::new("alpha");
    let input = sample_transparent_png(&bins, &tmp);
    let media = probe(&bins, &input).await.unwrap();
    assert!(media.picture.as_ref().unwrap().alpha);

    let (_, jpg) = convert(
        &bins,
        &tmp,
        &input,
        req(TargetFormat::Jpg, QualityPreset::Max),
    )
    .await;
    let jpg_path = tmp.path("out").join("Logo trasparente.jpg");
    assert!(!jpg.picture.unwrap().alpha);
    let [r, g, b, _] = pixel(&bins, &jpg_path, 5, 24);
    assert!(
        r > 245 && g > 245 && b > 245,
        "transparent → white, got {r} {g} {b}"
    );
    let [r, g, b, _] = pixel(&bins, &jpg_path, 50, 24);
    assert!(
        r < 40 && g < 40 && b > 200,
        "opaque blue kept, got {r} {g} {b}"
    );

    let (webp_out, webp) = convert(
        &bins,
        &tmp,
        &input,
        req(TargetFormat::Webp, QualityPreset::Balanced),
    )
    .await;
    assert!(webp.picture.unwrap().alpha);
    assert!(pixel(&bins, &webp_out.output, 5, 24)[3] < 10);
    assert!(pixel(&bins, &webp_out.output, 50, 24)[3] > 245);

    // AVIF keeps transparency in a second picture; Konvert merges it back.
    let (avif_out, avif) = convert(
        &bins,
        &tmp,
        &input,
        req(TargetFormat::Avif, QualityPreset::Balanced),
    )
    .await;
    assert!(avif.picture.unwrap().alpha_stream.is_some());
    let (png_out, png) = convert(
        &bins,
        &tmp,
        &avif_out.output,
        req(TargetFormat::Png, QualityPreset::Max),
    )
    .await;
    assert!(png.picture.unwrap().alpha);
    assert!(pixel(&bins, &png_out.output, 5, 24)[3] < 10);
    assert!(pixel(&bins, &png_out.output, 50, 24)[3] > 245);
}

#[tokio::test]
async fn heic_photos_are_assembled_from_their_tiles() {
    let bins = engine();
    let tmp = TempDir::new("heic");
    let input = sample_heic(&bins, &tmp, "IMG_0002.HEIC", 0);
    let media = probe(&bins, &input).await.unwrap();
    let picture = media.picture.as_ref().unwrap();
    assert_eq!(picture.grid_group, Some(0));
    assert!(picture.exif);
    let (outcome, _) = convert(
        &bins,
        &tmp,
        &input,
        req(TargetFormat::Png, QualityPreset::Max),
    )
    .await;
    let near = |[r, g, b, _]: [u8; 4], want: [u8; 3]| {
        [r, g, b].iter().zip(want).all(|(c, w)| c.abs_diff(w) < 40)
    };
    for (x, y, color) in [
        (32, 24, [255, 0, 0]),
        (96, 24, [0, 128, 0]),
        (32, 72, [0, 0, 255]),
        (96, 72, [255, 255, 255]),
    ] {
        let p = pixel(&bins, &outcome.output, x, y);
        assert!(near(p, color), "tile at {x},{y}: {p:?}, expected {color:?}");
    }

    // Rotated by its `irot` property (the EXIF orientation must not add a turn),
    // then limited to 64 pixels.
    let rotated = sample_heic(&bins, &tmp, "IMG_0003.HEIC", 1);
    let media = probe(&bins, &rotated).await.unwrap();
    assert_eq!(size_of(&media), (Some(96), Some(128)));
    let mut request = req(TargetFormat::Jpg, QualityPreset::Balanced);
    request.advanced.max_dimension = Some(64);
    let (_, out) = convert(&bins, &tmp, &rotated, request).await;
    assert_eq!(size_of(&out), (Some(48), Some(64)));
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
