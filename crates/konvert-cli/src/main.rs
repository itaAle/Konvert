//! `konvert-cli`: drives the conversion engine from a terminal.
//!
//! Used during development and by integration tests; it is not part of the
//! installed application.
//!
//! ```text
//! konvert-cli [--ffmpeg-dir DIR] version
//! konvert-cli [--ffmpeg-dir DIR] probe <FILE>
//! konvert-cli [--ffmpeg-dir DIR] convert <FILE> --to <FORMAT> [--preset max|balanced|small] [--out-dir DIR]
//! ```
//!
//! The engine directory defaults to `$KONVERT_FFMPEG_DIR`, then to
//! `src-tauri/binaries/ffmpeg` in this repository (filled by `scripts/setup-ffmpeg.*`).

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use konvert_core::{
    Binaries, ConversionJob, ConversionRequest, CoreError, QualityPreset, TargetFormat, Workspace,
    engine_version, probe, run_conversion, workspace,
};
use tokio_util::sync::CancellationToken;

const USAGE: &str = "usage:
  konvert-cli [--ffmpeg-dir DIR] version
  konvert-cli [--ffmpeg-dir DIR] probe <FILE>
  konvert-cli [--ffmpeg-dir DIR] convert <FILE> --to <FORMAT> [--preset max|balanced|small] [--out-dir DIR]";

enum Action {
    Version,
    Probe(PathBuf),
    Convert {
        input: PathBuf,
        target: TargetFormat,
        preset: QualityPreset,
        out_dir: Option<PathBuf>,
    },
}

struct Cli {
    ffmpeg_dir: PathBuf,
    action: Action,
}

fn parse_args() -> Result<Cli, String> {
    let mut args = std::env::args_os().skip(1);
    let mut ffmpeg_dir = None;
    let mut positional: Vec<std::ffi::OsString> = Vec::new();
    let mut target = None;
    let mut preset = QualityPreset::Balanced;
    let mut out_dir = None;

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
    let action = match positional.next().as_ref().and_then(|a| a.to_str()) {
        Some("version") => Action::Version,
        Some("probe") => Action::Probe(positional.next().ok_or("missing <FILE>")?.into()),
        Some("convert") => Action::Convert {
            input: positional.next().ok_or("missing <FILE>")?.into(),
            target: target.ok_or("missing --to <FORMAT>")?,
            preset,
            out_dir,
        },
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

async fn run(cli: Cli) -> Result<(), CoreError> {
    let bins = Binaries::from_dir(&cli.ffmpeg_dir)?;
    match cli.action {
        Action::Version => println!("{}", engine_version(&bins).await?),
        Action::Probe(input) => {
            let info = probe(&bins, &input).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&info).expect("serializable")
            );
        }
        Action::Convert {
            input,
            target,
            preset,
            out_dir,
        } => {
            let media = probe(&bins, &input).await?;
            let output_dir = match out_dir {
                Some(dir) => dir,
                None => input
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(".")),
            };
            let job = ConversionJob {
                id: 1,
                input,
                media,
                request: ConversionRequest { target, preset },
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
