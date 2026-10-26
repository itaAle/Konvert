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
//!
//! OPTIONS: --preset max|balanced|small  --height N  --fps N
//!          --audio-bitrate KBPS  --sample-rate HZ  --mono | --stereo
//!          --quality 1-100  --max-size PIXELS   (pictures)
//! ```
//!
//! The engine directory defaults to `$KONVERT_FFMPEG_DIR`, then to
//! `src-tauri/binaries/ffmpeg` in this repository (filled by `scripts/setup-ffmpeg.*`).

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use konvert_core::{
    AdvancedOptions, Binaries, ChannelLayout, ConversionJob, ConversionRequest, CoreError,
    QualityPreset, TargetFormat, Workspace, available_targets, engine_version, plan_conversion,
    probe, run_conversion, workspace,
};
use tokio_util::sync::CancellationToken;

const USAGE: &str = "usage:
  konvert-cli [--ffmpeg-dir DIR] version
  konvert-cli [--ffmpeg-dir DIR] probe   <FILE>
  konvert-cli [--ffmpeg-dir DIR] targets <FILE>
  konvert-cli [--ffmpeg-dir DIR] plan    <FILE> --to <FORMAT> [OPTIONS]
  konvert-cli [--ffmpeg-dir DIR] convert <FILE> --to <FORMAT> [OPTIONS] [--out-dir DIR]

OPTIONS: --preset max|balanced|small  --height N  --fps N
         --audio-bitrate KBPS  --sample-rate HZ  --mono | --stereo
         --quality 1-100  --max-size PIXELS   (pictures)";

enum Action {
    Version,
    Probe(PathBuf),
    Targets(PathBuf),
    Plan(PathBuf, ConversionRequest),
    Convert(PathBuf, ConversionRequest, Option<PathBuf>),
}

struct Cli {
    ffmpeg_dir: PathBuf,
    action: Action,
}

fn parse_args() -> Result<Cli, String> {
    let mut args = std::env::args_os().skip(1);
    let mut ffmpeg_dir = None;
    let mut positional: Vec<OsString> = Vec::new();
    let mut target = None;
    let mut preset = QualityPreset::Balanced;
    let mut advanced = AdvancedOptions::default();
    let mut out_dir = None;

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
    let mut file = || -> Result<PathBuf, String> {
        positional
            .next()
            .map(PathBuf::from)
            .ok_or("missing <FILE>".into())
    };
    let request = || -> Result<ConversionRequest, String> {
        Ok(ConversionRequest {
            target: target.ok_or("missing --to <FORMAT>")?,
            preset,
            advanced: advanced.clone(),
        })
    };
    let action = match command.as_ref().and_then(|c| c.to_str()) {
        Some("version") => Action::Version,
        Some("probe") => Action::Probe(file()?),
        Some("targets") => Action::Targets(file()?),
        Some("plan") => Action::Plan(file()?, request()?),
        Some("convert") => Action::Convert(file()?, request()?, out_dir),
        _ => return Err(String::new()),
    };
    Ok(Cli { ffmpeg_dir, action })
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
        Action::Targets(input) => {
            let media = probe(&bins, &input).await?;
            print_json(&available_targets(&media, &input));
        }
        Action::Plan(input, request) => {
            let media = probe(&bins, &input).await?;
            print_json(&plan_conversion(&media, &request, &input)?);
        }
        Action::Convert(input, request, out_dir) => {
            let media = probe(&bins, &input).await?;
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
            };
            let ws = Workspace::create(&workspace::default_base_dir())?;

            // Ctrl+C cancels the job (and cleans up the partial file).
            let cancel = CancellationToken::new();
            let on_ctrl_c = cancel.clone();
            tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    on_ctrl_c.cancel();
                }
            });

            let outcome = run_conversion(&bins, &ws, &job, &cancel, |p| {
                let pct = p.fraction.map(|f| format!("{:5.1}%", f * 100.0));
                let eta = p.eta_secs.map(|s| format!("ETA {s:.0}s"));
                let speed = p.speed.map(|s| format!("{s:.1}x"));
                let parts: Vec<String> = [pct, speed, eta].into_iter().flatten().collect();
                eprint!("\r{:<40}", parts.join("  "));
                let _ = std::io::stderr().flush();
            })
            .await;
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
            println!(
                "{} ({} bytes, {:.1}s)",
                outcome.output.display(),
                outcome.size_bytes,
                outcome.elapsed.as_secs_f64()
            );
        }
    }
    Ok(())
}
