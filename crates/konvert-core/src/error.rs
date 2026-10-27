//! Error type shared by the whole core crate.
//!
//! Every variant maps to a stable, machine-readable [`CoreError::code`] that the
//! UI translates into a human message. Raw FFmpeg output is never part of the
//! message itself: it travels separately in `details` so the UI can show it only
//! inside an expandable "technical details" section.

use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// The bundled FFmpeg/FFprobe executables could not be found.
    #[error("FFmpeg/FFprobe executables not found")]
    EngineMissing,

    /// The input path does not exist or is not a regular file.
    #[error("input file not found")]
    InputNotFound,

    /// The destination folder does not exist (e.g. a removed USB drive).
    #[error("destination folder not available")]
    OutputUnavailable,

    /// The file was readable but contains no audio, video or picture.
    #[error("the file contains no audio, video or picture")]
    NotMedia,

    /// FFmpeg/FFprobe could not decode the file (truncated, damaged, ...).
    #[error("the file seems to be damaged or is not a supported media file")]
    CorruptInput { details: String },

    /// The requested target cannot be produced from this input.
    #[error("this conversion is not supported")]
    UnsupportedConversion { details: String },

    /// The pages chosen in a PDF do not exist or cannot be read (`"1-3, 5"`).
    #[error("invalid page range")]
    InvalidPages { details: String },

    /// The PDF needs a password to be opened, and none was given.
    #[error("the PDF is protected by a password")]
    PdfEncrypted,

    /// The password given does not open the PDF.
    #[error("the password does not open the PDF")]
    PdfWrongPassword,

    /// The destination volume ran out of space.
    #[error("not enough disk space")]
    DiskFull { details: String },

    /// The OS denied access to the input or destination.
    #[error("permission denied")]
    PermissionDenied { details: String },

    /// The user cancelled the job.
    #[error("conversion cancelled")]
    Cancelled,

    /// A smaller copy would not be smaller than the file (already compressed).
    #[error("the file cannot be made smaller")]
    NoGain,

    /// FFmpeg failed for a reason we could not classify.
    #[error("the conversion engine reported an error")]
    EngineFailed { details: String },

    /// Any other I/O error (spawning processes, moving files, ...).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl CoreError {
    /// Stable identifier used by the UI to pick a translated message.
    pub fn code(&self) -> &'static str {
        match self {
            CoreError::EngineMissing => "engine_missing",
            CoreError::InputNotFound => "input_not_found",
            CoreError::OutputUnavailable => "output_unavailable",
            CoreError::NotMedia => "not_media",
            CoreError::CorruptInput { .. } => "corrupt_input",
            CoreError::UnsupportedConversion { .. } => "unsupported_conversion",
            CoreError::InvalidPages { .. } => "invalid_pages",
            CoreError::PdfEncrypted => "pdf_encrypted",
            CoreError::PdfWrongPassword => "pdf_wrong_password",
            CoreError::DiskFull { .. } => "disk_full",
            CoreError::PermissionDenied { .. } => "permission_denied",
            CoreError::Cancelled => "cancelled",
            CoreError::NoGain => "no_gain",
            CoreError::EngineFailed { .. } => "engine_failed",
            CoreError::Io(e) if is_disk_full(e) => "disk_full",
            CoreError::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                "permission_denied"
            }
            CoreError::Io(_) => "io",
        }
    }

    /// Technical details (FFmpeg stderr tail or OS error), if any.
    pub fn details(&self) -> Option<String> {
        match self {
            CoreError::CorruptInput { details }
            | CoreError::UnsupportedConversion { details }
            | CoreError::InvalidPages { details }
            | CoreError::DiskFull { details }
            | CoreError::PermissionDenied { details }
            | CoreError::EngineFailed { details } => (!details.is_empty()).then(|| details.clone()),
            CoreError::Io(e) => Some(e.to_string()),
            _ => None,
        }
    }

    /// Serializable form sent to the UI.
    pub fn payload(&self) -> ErrorPayload {
        ErrorPayload {
            code: self.code().to_owned(),
            details: self.details(),
        }
    }

    /// Classifies a failed FFmpeg/FFprobe run from the tail of its stderr.
    pub fn from_engine_output(stderr_tail: &str) -> Self {
        let lower = stderr_tail.to_lowercase();
        let details = stderr_tail.trim().to_owned();
        let has = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));

        if has(&["no space left on device", "not enough space on the disk"]) {
            CoreError::DiskFull { details }
        } else if has(&["permission denied", "access is denied"]) {
            CoreError::PermissionDenied { details }
        } else if has(&[
            "unknown encoder",
            "encoder not found",
            "not currently supported in container",
            "could not find tag for codec",
            "codec not currently supported",
            "incompatible with output",
        ]) {
            CoreError::UnsupportedConversion { details }
        } else if has(&[
            "invalid data found when processing input",
            "moov atom not found",
            "header missing",
            "error while decoding",
            "could not find codec parameters",
        ]) {
            CoreError::CorruptInput { details }
        } else {
            CoreError::EngineFailed { details }
        }
    }
}

fn is_disk_full(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::StorageFull | std::io::ErrorKind::QuotaExceeded
    )
}

/// What the UI receives when an operation fails.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct ErrorPayload {
    pub code: String,
    pub details: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_engine_output() {
        let cases = [
            (
                "x.wav: Invalid data found when processing input",
                "corrupt_input",
            ),
            (
                "Error writing trailer: No space left on device",
                "disk_full",
            ),
            (
                "Could not open file: Permission denied",
                "permission_denied",
            ),
            ("Unknown encoder 'libfoo'", "unsupported_conversion"),
            ("something unexpected happened", "engine_failed"),
        ];
        for (stderr, code) in cases {
            assert_eq!(
                CoreError::from_engine_output(stderr).code(),
                code,
                "{stderr}"
            );
        }
    }

    #[test]
    fn details_are_kept_separately_from_the_message() {
        let err = CoreError::from_engine_output("  moov atom not found\n");
        assert_eq!(err.details().as_deref(), Some("moov atom not found"));
        assert!(!err.to_string().contains("moov"));
    }
}
