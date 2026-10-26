//! Execution of a conversion job.
//!
//! Flow: build arguments → run FFmpeg writing into the private workspace →
//! report progress → on success move the file to its destination under a
//! unique name; on failure or cancellation delete the partial output.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::binaries::Binaries;
use crate::command::{build_from_plan, build_from_plan_with, file_url};
use crate::error::CoreError;
use crate::exif;
use crate::formats;
use crate::metadata;
use crate::naming;
use crate::plan::{ConversionPlan, plan_conversion};
use crate::probe::MediaInfo;
use crate::process;
use crate::progress::{Progress, ProgressParser};
use crate::request::{ConversionRequest, FrameMode};
use crate::svg;
use crate::workspace::Workspace;

/// Lines of FFmpeg stderr kept for the "technical details" section.
const STDERR_TAIL_LINES: usize = 40;
/// Longest stderr line kept (FFmpeg can print very long lines).
const STDERR_MAX_LINE: usize = 500;

/// Everything needed to run one conversion.
#[derive(Debug, Clone)]
pub struct ConversionJob {
    /// Identifier unique within this run (used for the temporary file name).
    pub id: u64,
    pub input: PathBuf,
    pub media: MediaInfo,
    pub request: ConversionRequest,
    /// Folder where the finished file is placed.
    pub output_dir: PathBuf,
}

/// Result of a successful conversion.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConversionOutcome {
    /// The converted file, or the folder holding a series of pictures.
    pub output: PathBuf,
    /// Total size of the produced files.
    pub size_bytes: u64,
    /// Number of files produced (more than one: `output` is a folder).
    pub files: u32,
    /// Seconds.
    #[serde(serialize_with = "serialize_secs")]
    #[ts(type = "number")]
    pub elapsed: Duration,
}

fn serialize_secs<S: serde::Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(d.as_secs_f64())
}

/// Runs `job`, calling `on_progress` as FFmpeg reports progress.
pub async fn run_conversion(
    bins: &Binaries,
    workspace: &Workspace,
    job: &ConversionJob,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(Progress) + Send,
) -> Result<ConversionOutcome, CoreError> {
    let started = Instant::now();
    if !job.input.is_file() {
        return Err(CoreError::InputNotFound);
    }
    if !job.output_dir.is_dir() {
        return Err(CoreError::OutputUnavailable);
    }
    let plan = plan_conversion(&job.media, &job.request, &job.input)?;
    let ext = formats::spec(job.request.target).extension;
    if plan.frames.is_some_and(|f| f.mode == FrameMode::Sequence) {
        return run_series(bins, workspace, job, &plan, cancel, on_progress, started).await;
    }
    let temp_output = workspace.job_output(job.id, ext);
    let cover_comment = prepare_cover_comment(bins, workspace, job, &plan).await;
    // A vector picture is drawn first; FFmpeg then encodes the drawing.
    let drawing = match plan.image.as_ref().and_then(|i| i.render) {
        Some((width, height)) => Some(render_drawing(workspace, job, width, height).await?),
        None => None,
    };
    let source = drawing.as_deref().unwrap_or(&job.input);
    let command = build_from_plan_with(&plan, source, cover_comment.as_deref(), &temp_output);

    let result = run_ffmpeg(
        bins,
        &command.args,
        job.media.duration_secs,
        cancel,
        &mut on_progress,
    )
    .await;
    for temporary in [&cover_comment, &drawing].into_iter().flatten() {
        let _ = std::fs::remove_file(temporary);
    }
    let result = result.and_then(|()| {
        // FFmpeg can succeed without writing anything (e.g. no frame there).
        match std::fs::metadata(&temp_output) {
            Ok(m) if m.len() > 0 => Ok(()),
            _ => Err(nothing_written()),
        }
    });

    if let Err(e) = result {
        let _ = std::fs::remove_file(&temp_output);
        return Err(e);
    }

    if let Some(image) = plan
        .image
        .as_ref()
        .filter(|i| i.copy_exif || plan.remove_location)
    {
        let (input, output) = (job.input.clone(), temp_output.clone());
        let (copy, remove, size) = (
            image.copy_exif,
            plan.remove_location,
            image.width.zip(image.height),
        );
        let _ =
            tokio::task::spawn_blocking(move || finish_exif(&input, &output, copy, remove, size))
                .await;
    }

    let stem = output_stem(&job.input);
    let output_dir = job.output_dir.clone();
    let temp = temp_output.clone();
    let moved =
        tokio::task::spawn_blocking(move || naming::move_to_unique(&temp, &output_dir, &stem, ext))
            .await
            .map_err(|e| CoreError::Io(std::io::Error::other(e)))?;

    let output = match moved {
        Ok(path) => path,
        Err(e) => {
            let _ = std::fs::remove_file(&temp_output);
            return Err(e.into());
        }
    };
    let size_bytes = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
    Ok(ConversionOutcome {
        output,
        size_bytes,
        files: 1,
        elapsed: started.elapsed(),
    })
}

/// A series of pictures: FFmpeg writes them into a private folder, then they
/// move into a new folder named after the input, next to the other outputs.
async fn run_series(
    bins: &Binaries,
    workspace: &Workspace,
    job: &ConversionJob,
    plan: &ConversionPlan,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(Progress) + Send,
    started: Instant,
) -> Result<ConversionOutcome, CoreError> {
    let ext = formats::spec(job.request.target).extension;
    let temp_dir = workspace.job_dir(job.id);
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir)?;
    let command = build_from_plan(plan, &job.input, &series_pattern(&temp_dir, ext));

    let result = run_ffmpeg(
        bins,
        &command.args,
        job.media.duration_secs,
        cancel,
        &mut on_progress,
    )
    .await;
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(e);
    }

    let stem = output_stem(&job.input);
    let output_dir = job.output_dir.clone();
    let temp = temp_dir.clone();
    let moved =
        tokio::task::spawn_blocking(move || naming::move_series(&temp, &output_dir, &stem, ext))
            .await
            .map_err(|e| CoreError::Io(std::io::Error::other(e)));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let (output, files, size_bytes) = moved??;
    if files == 0 {
        let _ = std::fs::remove_dir(&output);
        return Err(nothing_written());
    }
    Ok(ConversionOutcome {
        output,
        size_bytes,
        files,
        elapsed: started.elapsed(),
    })
}

/// Draws an SVG picture at its planned size into a PNG file of the private
/// workspace.
async fn render_drawing(
    workspace: &Workspace,
    job: &ConversionJob,
    width: u32,
    height: u32,
) -> Result<PathBuf, CoreError> {
    let input = job.input.clone();
    let png = tokio::task::spawn_blocking(move || svg::render_png(&input, width, height))
        .await
        .map_err(|e| CoreError::Io(std::io::Error::other(e)))??;
    let path = workspace.dir().join(format!("job-{}-drawing.png", job.id));
    std::fs::write(&path, png)?;
    Ok(path)
}

/// Copies the photo's EXIF data into the converted picture, still in the
/// private workspace. When the location must go and the data cannot be
/// copied, any EXIF data FFmpeg wrote is removed instead. If something fails,
/// the picture stays as FFmpeg wrote it.
fn finish_exif(
    input: &Path,
    output: &Path,
    copy: bool,
    remove_location: bool,
    size: Option<(u32, u32)>,
) {
    let Ok(converted) = std::fs::read(output) else {
        return;
    };
    let copied = copy
        .then(|| {
            let tiff = exif::extract(&std::fs::read(input).ok()?)?;
            let tiff = exif::prepare(&tiff, remove_location, size)?;
            exif::insert(&converted, &tiff).ok()
        })
        .flatten();
    let result = copied.or_else(|| remove_location.then(|| exif::strip(&converted)).flatten());
    if let Some(bytes) = result {
        let _ = std::fs::write(output, bytes);
    }
}

/// Ogg outputs keep the cover as a picture comment: the picture is copied out
/// of the input, then written into an `ffmetadata` file for FFmpeg. `None`
/// when there is nothing to do or the picture cannot be read (the conversion
/// then goes on without it).
async fn prepare_cover_comment(
    bins: &Binaries,
    workspace: &Workspace,
    job: &ConversionJob,
    plan: &ConversionPlan,
) -> Option<PathBuf> {
    let spec = formats::spec(plan.target);
    let index = plan.cover_art_stream.filter(|_| spec.cover_art_tag)?;
    let cover = job.media.cover_art.as_ref().filter(|c| c.index == index)?;
    let mime = metadata::cover_mime(&cover.codec)?;

    let picture = workspace.dir().join(format!("job-{}-cover", job.id));
    let mut cmd = process::engine_command(&bins.ffmpeg);
    cmd.args(["-hide_banner", "-nostdin", "-loglevel", "error"])
        .args(["-protocol_whitelist", "file", "-i"])
        .arg(file_url(&job.input))
        .args([
            "-map",
            &format!("0:{index}"),
            "-c",
            "copy",
            "-frames:v",
            "1",
        ])
        .args(["-f", "image2pipe", "-y"])
        .arg(file_url(&picture));
    let extracted = match process::spawn(&mut cmd) {
        Ok(child) => child
            .wait_with_output()
            .await
            .is_ok_and(|o| o.status.success()),
        Err(_) => false,
    };
    let data = std::fs::read(&picture).ok();
    let _ = std::fs::remove_file(&picture);
    let data = data.filter(|d| extracted && !d.is_empty())?;

    let block = metadata::picture_block(
        mime,
        cover.width.unwrap_or(0),
        cover.height.unwrap_or(0),
        &data,
    );
    let comment = workspace.dir().join(format!("job-{}-cover.txt", job.id));
    std::fs::write(&comment, metadata::ffmetadata_with_picture(&block)).ok()?;
    Some(comment)
}

/// File name pattern of a series for FFmpeg's image2 muxer: `%` in the
/// folder name is escaped, `%06d` numbers the pictures.
fn series_pattern(dir: &Path, ext: &str) -> PathBuf {
    let dir = dir.to_string_lossy().replace('%', "%%");
    PathBuf::from(format!(
        "{dir}{}frame-%06d.{ext}",
        std::path::MAIN_SEPARATOR
    ))
}

fn nothing_written() -> CoreError {
    CoreError::EngineFailed {
        details: "the conversion produced no output".into(),
    }
}

/// Output base name: the input's file name without extension.
fn output_stem(input: &Path) -> OsString {
    input
        .file_stem()
        .map(|s| s.to_os_string())
        .unwrap_or_else(|| OsString::from("output"))
}

async fn run_ffmpeg(
    bins: &Binaries,
    args: &[OsString],
    duration_secs: Option<f64>,
    cancel: &CancellationToken,
    on_progress: &mut (impl FnMut(Progress) + Send),
) -> Result<(), CoreError> {
    let mut cmd = process::engine_command(&bins.ffmpeg);
    cmd.args(args);
    let mut child = process::spawn(&mut cmd)?;

    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let stderr_task = tokio::spawn(collect_tail(stderr));

    let mut lines = BufReader::new(stdout).lines();
    let mut parser = ProgressParser::new();
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                stderr_task.abort();
                return Err(CoreError::Cancelled);
            }
            line = lines.next_line() => match line {
                Ok(Some(line)) => {
                    if let Some(snapshot) = parser.feed_line(&line) {
                        on_progress(Progress::from_snapshot(&snapshot, duration_secs));
                    }
                }
                // stdout closed: FFmpeg is exiting.
                Ok(None) | Err(_) => break,
            },
        }
    }

    let status = tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            let _ = child.kill().await;
            stderr_task.abort();
            return Err(CoreError::Cancelled);
        }
        status = child.wait() => status?,
    };
    let stderr_tail = stderr_task.await.unwrap_or_default();

    if status.success() {
        Ok(())
    } else {
        Err(CoreError::from_engine_output(&stderr_tail))
    }
}

/// Keeps only the last lines of a stream, each truncated to a sane length.
async fn collect_tail(stream: impl AsyncRead + Unpin) -> String {
    let mut tail: VecDeque<String> = VecDeque::with_capacity(STDERR_TAIL_LINES);
    let mut lines = BufReader::new(stream).lines();
    while let Ok(Some(mut line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > STDERR_MAX_LINE {
            let cut = (0..=STDERR_MAX_LINE)
                .rev()
                .find(|i| line.is_char_boundary(*i))
                .unwrap_or(0);
            line.truncate(cut);
            line.push('…');
        }
        if tail.len() == STDERR_TAIL_LINES {
            tail.pop_front();
        }
        tail.push_back(line);
    }
    Vec::from(tail).join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_stem_keeps_unicode_names() {
        assert_eq!(
            output_stem(Path::new("Musica/Perché sì — live.wav")),
            OsString::from("Perché sì — live")
        );
    }
}
