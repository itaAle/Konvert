//! Konvert core: everything that does not depend on the user interface.
//!
//! * [`probe`]: media analysis with FFprobe.
//! * [`command`]: pure FFmpeg command builder.
//! * [`convert`]: runs FFmpeg, reports progress, supports cancellation.
//! * [`naming`] / [`workspace`]: safe output naming and temporary files.
//!
//! Privacy: this crate only ever reads and writes local files. FFmpeg and FFprobe
//! are invoked with `-protocol_whitelist file`, so they cannot open network URLs.

pub mod binaries;
pub mod command;
pub mod convert;
pub mod error;
pub mod naming;
pub mod probe;
pub mod progress;
pub mod workspace;

mod process;

pub use binaries::Binaries;
pub use command::{ConversionRequest, QualityPreset, TargetFormat};
pub use convert::{ConversionJob, ConversionOutcome, run_conversion};
pub use error::{CoreError, ErrorPayload};
pub use probe::{MediaInfo, MediaKind, probe};
pub use progress::Progress;
pub use workspace::Workspace;

/// First line of `ffmpeg -version` (e.g. `ffmpeg version n9.0.2 ...`).
pub async fn engine_version(bins: &Binaries) -> Result<String, CoreError> {
    let mut cmd = process::engine_command(&bins.ffmpeg);
    cmd.args(["-hide_banner", "-version"]);
    let output = process::spawn(&mut cmd)?.wait_with_output().await?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .next()
        .map(|l| l.trim().to_owned())
        .filter(|l| output.status.success() && !l.is_empty())
        .ok_or(CoreError::EngineMissing)
}
