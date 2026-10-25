//! Konvert core: everything that does not depend on the user interface.
//!
//! * [`probe`]: media analysis with FFprobe.
//! * [`formats`]: declarative catalog of output formats and proposal rules.
//! * [`request`]: what the user asks for (format, preset, advanced options).
//! * [`plan`]: decisions for one conversion (copy/re-encode, codecs, warnings).
//! * [`estimate`]: estimated output size.
//! * [`command`]: pure FFmpeg command builder (plan → arguments).
//! * [`convert`]: runs FFmpeg, reports progress, supports cancellation.
//! * [`naming`] / [`workspace`]: safe output naming and temporary files.
//!
//! Privacy: this crate only ever reads and writes local files. FFmpeg and FFprobe
//! are invoked with `-protocol_whitelist file`, so they cannot open network URLs.

pub mod binaries;
pub mod command;
pub mod convert;
pub mod disk;
pub mod error;
pub mod estimate;
pub mod formats;
pub mod naming;
pub mod plan;
pub mod probe;
pub mod progress;
pub mod request;
pub mod workspace;

mod process;

pub use binaries::Binaries;
pub use convert::{ConversionJob, ConversionOutcome, run_conversion};
pub use disk::available_space;
pub use error::{CoreError, ErrorPayload};
pub use estimate::SizeEstimate;
pub use formats::{TargetFormat, TargetOption, available_targets};
pub use plan::{ConversionPlan, ConversionWarning, plan_conversion};
pub use probe::{MediaInfo, MediaKind, probe};
pub use progress::Progress;
pub use request::{AdvancedOptions, ChannelLayout, ConversionRequest, QualityPreset};
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
