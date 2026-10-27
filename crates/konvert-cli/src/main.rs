//! `konvert-cli`: drives the conversion engine from a terminal.
//!
//! Used during development and for manual checks; it is not part of the
//! installed application.
//!
//! ```text
//! konvert-cli [--ffmpeg-dir DIR] version
//! konvert-cli [--ffmpeg-dir DIR] probe   <FILE>
//! konvert-cli [--ffmpeg-dir DIR] targets <FILE>
//! konvert-cli [--ffmpeg-dir DIR] plan    <FILE> --to <FORMAT> [OPTIONS]
//! konvert-cli [--ffmpeg-dir DIR] convert <FILE> --to <FORMAT> [OPTIONS] [--out-dir DIR]
//! konvert-cli [--ffmpeg-dir DIR] combine <FILE>... [OPTIONS] [--out-dir DIR]   (one PDF)
//! konvert-cli fonts <FILE.pdf>   (embedded fonts and how much Compress reduces them)
//!
//! OPTIONS: --preset max|balanced|small  --height N  --fps N
//!          --audio-bitrate KBPS  --sample-rate HZ  --mono | --stereo
//!          --quality 1-100  --max-size PIXELS   (pictures)
//!          --frame-at SECONDS | --every SECONDS | --every-frame   (pictures from a video)
//!          --remove-location   (erase GPS data and location tags)
//!          --page-size a4|letter|fit  --orientation auto|portrait|landscape
//!          --margin none|small|normal   (pictures to PDF)
//!          --dpi N  --pages "1-3, 5"   (PDF to pictures)
//!          --layout "3, 1-2, 5@90"  --rotate 90|180|270
//!          --split-every N | --split-after "2, 5"   (PDF to PDF; with combine,
//!                    "2:1-3" are pages of the second file)
//!          --compress light|strong|max  --max-mb N  --pages-as-pictures
//!                    (a smaller copy in the same format: no --to needed)
//! ```
//!
//! The engine directory defaults to `$KONVERT_FFMPEG_DIR`, then to
//! `src-tauri/binaries/ffmpeg` in this repository (filled by `scripts/setup-ffmpeg.*`).

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use konvert_core::{
    AdvancedOptions, Binaries, ChannelLayout, CombinedJob, CompressLevel, CompressOptions,
    ConversionJob, ConversionRequest, CoreError, FrameMode, FrameOptions, MediaInfo, PageMargin,
    PageOrientation, PageSize, PdfOptions, PdfPage, PdfSplit, Progress, QualityPreset,
    TargetFormat, Workspace, available_targets, compress::compress_format, engine_encoders,
    engine_version, formats::supported_targets, plan_conversion, probe, run_combined,
    run_conversion, workspace,
};
use tokio_util::sync::CancellationToken;

const USAGE: &str = "usage:
  konvert-cli [--ffmpeg-dir DIR] version
  konvert-cli [--ffmpeg-dir DIR] probe   <FILE>
  konvert-cli [--ffmpeg-dir DIR] targets <FILE>
  konvert-cli [--ffmpeg-dir DIR] plan    <FILE> --to <FORMAT> [OPTIONS]
  konvert-cli [--ffmpeg-dir DIR] convert <FILE> --to <FORMAT> [OPTIONS] [--out-dir DIR]
  konvert-cli [--ffmpeg-dir DIR] combine <FILE>... [OPTIONS] [--out-dir DIR]   (one PDF)
  konvert-cli fonts <FILE.pdf>   (embedded fonts and how much Compress reduces them)

OPTIONS: --preset max|balanced|small  --height N  --fps N
         --audio-bitrate KBPS  --sample-rate HZ  --mono | --stereo
         --quality 1-100  --max-size PIXELS   (pictures)
         --frame-at SECONDS | --every SECONDS | --every-frame   (pictures from a video)
         --remove-location   (erase GPS data and location tags)
         --page-size a4|letter|fit  --orientation auto|portrait|landscape
         --margin none|small|normal   (pictures to PDF)
         --dpi N  --pages \"1-3, 5\"   (PDF to pictures)
         --layout \"3, 1-2, 5@90\"  --rotate 90|180|270
         --split-every N | --split-after \"2, 5\"   (PDF to PDF; with combine,
                   \"2:1-3\" are pages of the second file)
         --compress light|strong|max  --max-mb N  --pages-as-pictures
                   (a smaller copy in the same format: no --to needed)";

enum Action {
    Version,
    Probe(PathBuf),
    Fonts(PathBuf),
    Targets(PathBuf),
    Plan(PathBuf, ConversionRequest),
    Convert(PathBuf, ConversionRequest, Option<PathBuf>),
    Combine(Vec<PathBuf>, ConversionRequest, Option<PathBuf>),
}

struct Cli {
    ffmpeg_dir: PathBuf,
    action: Action,
    /// `--rotate`: turn every page of the new PDF.
    rotate: Option<u16>,
}

/// `"3, 1-2, 5@90"`: pages in this order (a range may go backwards), each
/// group optionally turned clockwise by 90, 180 or 270 degrees.
fn parse_layout(text: &str) -> Result<Vec<PdfPage>, String> {
    let mut pages = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let wrong = || format!("invalid layout \"{part}\"");
        let (range, rotation) = match part.split_once('@') {
            Some((range, turn)) => (range, turn.trim().parse().map_err(|_| wrong())?),
            None => (part, 0),
        };
        // "2:1-3": pages of the second file combined.
        let (file, range) = match range.split_once(':') {
            Some((file, range)) => match file.trim().parse::<u32>() {
                Ok(file) if file > 0 => (file - 1, range),
                _ => return Err(wrong()),
            },
            None => (0, range),
        };
        let number = |s: &str| s.trim().parse::<u32>().map_err(|_| wrong());
        let (first, last) = match range.split_once('-') {
            Some((a, b)) => (number(a)?, number(b)?),
            None => (number(range)?, number(range)?),
        };
        let numbers: Vec<u32> = if first <= last {
            (first..=last).collect()
        } else {
            (last..=first).rev().collect()
        };
        pages.extend(numbers.into_iter().map(|number| PdfPage {
            file,
            number,
            rotation,
        }));
    }
    Ok(pages)
}

/// Lists the fonts embedded in a PDF, the largest first, with the size of
/// each program and what Compress would make of it.
fn print_fonts(input: &std::path::Path) -> Result<(), CoreError> {
    let data = std::fs::read(input)?;
    let mut doc = lopdf::Document::load_mem(&data).map_err(|e| CoreError::CorruptInput {
        details: e.to_string(),
    })?;
    let reports = konvert_core::pdf_fonts::subset_fonts(&mut doc);
    let (mut before, mut after) = (0, 0);
    for font in &reports {
        let reduced = font.reduced.map_or("kept as it is".to_string(), |n| {
            format!("-> {:>9} bytes", n)
        });
        println!(
            "{:>10} bytes  {:<9} {:<40} {reduced}",
            font.bytes, font.kind, font.name
        );
        before += font.bytes;
        after += font.reduced.unwrap_or(font.bytes);
    }
    println!(
        "{} fonts: {before} -> {after} bytes (uncompressed), file {} bytes",
        reports.len(),
        data.len()
    );
    Ok(())
}

/// Applies `--rotate` to every page of the new PDF (every page of the
/// document when no `--layout` was given).
fn rotate_pages(request: &mut ConversionRequest, media: &MediaInfo, rotate: Option<u16>) {
    let (Some(turn), Some(info)) = (rotate, media.document.as_ref()) else {
        return;
    };
    let pdf = request.pdf.get_or_insert_default();
    let layout = pdf.layout.get_or_insert_with(|| {
        (1..=info.pages)
            .map(|number| PdfPage {
                file: 0,
                number,
                rotation: 0,
            })
            .collect()
    });
    for page in layout {
        page.rotation = (page.rotation + turn) % 360;
    }
}

/// A smaller copy is written in the file's own format.
fn compressed_format(
    request: &mut ConversionRequest,
    media: &MediaInfo,
    input: &std::path::Path,
) -> Result<(), CoreError> {
    if request.compress.is_some() {
        request.target =
            compress_format(media, input).ok_or_else(|| CoreError::UnsupportedConversion {
                details: "Konvert cannot make this file smaller".into(),
            })?;
    }
    Ok(())
}

fn parse_args() -> Result<Cli, String> {
    let mut args = std::env::args_os().skip(1);
    let mut ffmpeg_dir = None;
    let mut positional: Vec<OsString> = Vec::new();
    let mut target = None;
    let mut preset = QualityPreset::Balanced;
    let mut advanced = AdvancedOptions::default();
    let mut out_dir = None;
    let mut frames: Option<FrameOptions> = None;
    let mut remove_location = false;
    let mut pdf: Option<PdfOptions> = None;
    let mut rotate = None;
    let mut compress: Option<CompressOptions> = None;

    fn number<T: std::str::FromStr>(name: &str, v: OsString) -> Result<T, String> {
        v.to_str()
            .and_then(|s| s.parse().ok())
            .ok_or(format!("{name} needs a number"))
    }

    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.to_str() {
            Some("--ffmpeg-dir") => ffmpeg_dir = Some(PathBuf::from(value("--ffmpeg-dir")?)),
            Some("--out-dir") => out_dir = Some(PathBuf::from(value("--out-dir")?)),
            Some("--to") => {
                let v = value("--to")?;
                let id = v.to_string_lossy();
                target = Some(TargetFormat::from_id(&id).ok_or(format!("unknown format: {id}"))?);
            }
            Some("--preset") => {
                preset = match value("--preset")?.to_str() {
                    Some("max") => QualityPreset::Max,
                    Some("balanced") => QualityPreset::Balanced,
                    Some("small") => QualityPreset::Small,
                    _ => return Err("preset must be max, balanced or small".into()),
                }
            }
            Some("--height") => advanced.max_height = Some(number("--height", value("--height")?)?),
            Some("--fps") => advanced.fps = Some(number("--fps", value("--fps")?)?),
            Some("--audio-bitrate") => {
                advanced.audio_bitrate_kbps =
                    Some(number("--audio-bitrate", value("--audio-bitrate")?)?)
            }
            Some("--sample-rate") => {
                advanced.sample_rate = Some(number("--sample-rate", value("--sample-rate")?)?)
            }
            Some("--quality") => {
                advanced.image_quality = Some(number("--quality", value("--quality")?)?)
            }
            Some("--max-size") => {
                advanced.max_dimension = Some(number("--max-size", value("--max-size")?)?)
            }
            Some("--frame-at") => {
                frames = Some(FrameOptions {
                    at_secs: Some(number("--frame-at", value("--frame-at")?)?),
                    ..FrameOptions::default()
                })
            }
            Some("--every") => {
                frames = Some(FrameOptions {
                    mode: FrameMode::Sequence,
                    every_secs: Some(number("--every", value("--every")?)?),
                    ..FrameOptions::default()
                })
            }
            Some("--every-frame") => {
                frames = Some(FrameOptions {
                    mode: FrameMode::Sequence,
                    ..FrameOptions::default()
                })
            }
            Some("--remove-location") => remove_location = true,
            Some("--page-size") => {
                pdf.get_or_insert_default().page_size = match value("--page-size")?.to_str() {
                    Some("a4") => PageSize::A4,
                    Some("letter") => PageSize::Letter,
                    Some("fit") => PageSize::Fit,
                    _ => return Err("page size must be a4, letter or fit".into()),
                }
            }
            Some("--orientation") => {
                pdf.get_or_insert_default().orientation = match value("--orientation")?.to_str() {
                    Some("auto") => PageOrientation::Auto,
                    Some("portrait") => PageOrientation::Portrait,
                    Some("landscape") => PageOrientation::Landscape,
                    _ => return Err("orientation must be auto, portrait or landscape".into()),
                }
            }
            Some("--margin") => {
                pdf.get_or_insert_default().margin = match value("--margin")?.to_str() {
                    Some("none") => PageMargin::None,
                    Some("small") => PageMargin::Small,
                    Some("normal") => PageMargin::Normal,
                    _ => return Err("margin must be none, small or normal".into()),
                }
            }
            Some("--dpi") => pdf.get_or_insert_default().dpi = number("--dpi", value("--dpi")?)?,
            Some("--pages") => {
                pdf.get_or_insert_default().pages =
                    Some(value("--pages")?.to_string_lossy().into_owned())
            }
            Some("--layout") => {
                pdf.get_or_insert_default().layout =
                    Some(parse_layout(&value("--layout")?.to_string_lossy())?)
            }
            Some("--rotate") => rotate = Some(number("--rotate", value("--rotate")?)?),
            Some("--split-every") => {
                let pdf = pdf.get_or_insert_default();
                pdf.split = PdfSplit::Every;
                pdf.split_every = number("--split-every", value("--split-every")?)?;
            }
            Some("--split-after") => {
                let pdf = pdf.get_or_insert_default();
                pdf.split = PdfSplit::After;
                pdf.split_after = value("--split-after")?
                    .to_string_lossy()
                    .split(',')
                    .map(|n| n.trim().parse())
                    .collect::<Result<_, _>>()
                    .map_err(|_| "--split-after needs page positions such as \"2, 5\"")?;
            }
            Some("--compress") => {
                compress.get_or_insert_default().level = match value("--compress")?.to_str() {
                    Some("light") => CompressLevel::Light,
                    Some("strong") => CompressLevel::Strong,
                    Some("max") => CompressLevel::Max,
                    _ => return Err("compression must be light, strong or max".into()),
                }
            }
            Some("--max-mb") => {
                let mb: f64 = number("--max-mb", value("--max-mb")?)?;
                compress.get_or_insert_default().max_bytes = Some((mb * 1_000_000.0) as u64);
            }
            Some("--pages-as-pictures") => {
                compress.get_or_insert_default().pages_as_pictures = true
            }
            Some("--mono") => advanced.channels = Some(ChannelLayout::Mono),
            Some("--stereo") => advanced.channels = Some(ChannelLayout::Stereo),
            Some("-h" | "--help") => return Err(String::new()),
            _ => positional.push(arg),
        }
    }

    let ffmpeg_dir = ffmpeg_dir
        .or_else(|| std::env::var_os("KONVERT_FFMPEG_DIR").map(PathBuf::from))
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/binaries/ffmpeg")
        });

    let mut positional = positional.into_iter();
    let command = positional.next();
    if command.as_ref().and_then(|c| c.to_str()) == Some("combine") {
        target.get_or_insert(TargetFormat::Pdf);
    }
    let mut file = || -> Result<PathBuf, String> {
        positional
            .next()
            .map(PathBuf::from)
            .ok_or("missing <FILE>".into())
    };
    let request = || -> Result<ConversionRequest, String> {
        Ok(ConversionRequest {
            // A smaller copy keeps the file's format, found once it is analysed.
            target: match (target, &compress) {
                (Some(target), _) => target,
                (None, Some(_)) => TargetFormat::Pdf,
                (None, None) => return Err("missing --to <FORMAT>".into()),
            },
            compress: compress.clone(),
            preset,
            advanced: advanced.clone(),
            frames,
            remove_location,
            pdf: pdf.clone(),
        })
    };
    let action = match command.as_ref().and_then(|c| c.to_str()) {
        Some("version") => Action::Version,
        Some("probe") => Action::Probe(file()?),
        Some("fonts") => Action::Fonts(file()?),
        Some("targets") => Action::Targets(file()?),
        Some("plan") => Action::Plan(file()?, request()?),
        Some("convert") => Action::Convert(file()?, request()?, out_dir),
        Some("combine") => {
            let files: Vec<PathBuf> = positional.map(PathBuf::from).collect();
            if files.is_empty() {
                return Err("missing <FILE>...".into());
            }
            Action::Combine(files, request()?, out_dir)
        }
        _ => return Err(String::new()),
    };
    Ok(Cli {
        ffmpeg_dir,
        action,
        rotate,
    })
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = match parse_args() {
        Ok(cli) => cli,
        Err(msg) => {
            if !msg.is_empty() {
                eprintln!("error: {msg}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error [{}]: {e}", e.code());
            if let Some(details) = e.details() {
                eprintln!("--- technical details ---\n{details}");
            }
            ExitCode::FAILURE
        }
    }
}

fn print_json(value: &impl serde::Serialize) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("serializable")
    );
}

async fn run(cli: Cli) -> Result<(), CoreError> {
    let bins = Binaries::from_dir(&cli.ffmpeg_dir)?;
    match cli.action {
        Action::Version => println!("{}", engine_version(&bins).await?),
        Action::Probe(input) => print_json(&probe(&bins, &input).await?),
        Action::Fonts(input) => print_fonts(&input)?,
        Action::Targets(input) => {
            let media = probe(&bins, &input).await?;
            let encoders = engine_encoders(&bins).await.unwrap_or_default();
            print_json(&supported_targets(
                available_targets(&media, &input),
                &encoders,
            ));
        }
        Action::Plan(input, mut request) => {
            let media = probe(&bins, &input).await?;
            rotate_pages(&mut request, &media, cli.rotate);
            compressed_format(&mut request, &media, &input)?;
            print_json(&plan_conversion(&media, &request, &input)?);
        }
        Action::Convert(input, mut request, out_dir) => {
            let media = probe(&bins, &input).await?;
            rotate_pages(&mut request, &media, cli.rotate);
            compressed_format(&mut request, &media, &input)?;
            let plan = plan_conversion(&media, &request, &input)?;
            for warning in &plan.warnings {
                eprintln!("warning: {warning:?}");
            }
            let output_dir = out_dir.unwrap_or_else(|| {
                input
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."))
            });
            let job = ConversionJob {
                id: 1,
                input,
                media,
                request,
                output_dir,
                passwords: Default::default(),
            };
            let ws = Workspace::create(&workspace::default_base_dir())?;
            let cancel = cancel_on_ctrl_c();
            let outcome = run_conversion(&bins, &ws, &job, &cancel, show_progress).await;
            eprintln!();
            let outcome = outcome?;
            if let Some(estimate) = plan.estimate {
                eprintln!(
                    "estimate: {} bytes{}",
                    estimate.bytes,
                    if estimate.approximate {
                        " (approx.)"
                    } else {
                        ""
                    }
                );
            }
            print_outcome(&outcome);
        }
        Action::Combine(inputs, request, out_dir) => {
            let mut probed = Vec::with_capacity(inputs.len());
            for input in inputs {
                let media = probe(&bins, &input).await?;
                probed.push((input, media));
            }
            let output_dir = out_dir.unwrap_or_else(|| {
                probed[0]
                    .0
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."))
            });
            let job = CombinedJob {
                id: 1,
                inputs: probed,
                request,
                output_dir,
                passwords: Default::default(),
            };
            let ws = Workspace::create(&workspace::default_base_dir())?;
            let cancel = cancel_on_ctrl_c();
            let outcome = run_combined(&bins, &ws, &job, &cancel, show_progress).await;
            eprintln!();
            print_outcome(&outcome?);
        }
    }
    Ok(())
}

/// Ctrl+C cancels the job (and cleans up the partial files).
fn cancel_on_ctrl_c() -> CancellationToken {
    let cancel = CancellationToken::new();
    let on_ctrl_c = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            on_ctrl_c.cancel();
        }
    });
    cancel
}

fn show_progress(p: Progress) {
    let pct = p.fraction.map(|f| format!("{:5.1}%", f * 100.0));
    let eta = p.eta_secs.map(|s| format!("ETA {s:.0}s"));
    let speed = p.speed.map(|s| format!("{s:.1}x"));
    let parts: Vec<String> = [pct, speed, eta].into_iter().flatten().collect();
    eprint!("\r{:<40}", parts.join("  "));
    let _ = std::io::stderr().flush();
}

fn print_outcome(outcome: &konvert_core::ConversionOutcome) {
    println!(
        "{} ({} file(s), {} bytes, {:.1}s)",
        outcome.output.display(),
        outcome.files,
        outcome.size_bytes,
        outcome.elapsed.as_secs_f64()
    );
}
