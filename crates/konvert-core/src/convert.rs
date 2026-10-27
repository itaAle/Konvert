//! Execution of a conversion job.
//!
//! Flow: build arguments → run FFmpeg writing into the private workspace →
//! report progress → on success move the file to its destination under a
//! unique name; on failure or cancellation delete the partial output.

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::binaries::Binaries;
use crate::command::{build_from_plan, build_from_plan_with, file_url};
use crate::compress;
use crate::docx;
use crate::error::CoreError;
use crate::exif;
use crate::formats::{self, TargetFormat};
use crate::metadata;
use crate::naming;
use crate::pdf;
use crate::pdf_compress;
use crate::plan::{ConversionPlan, plan_conversion};
use crate::probe::{MediaInfo, MediaKind};
use crate::process;
use crate::progress::{Progress, ProgressParser};
use crate::request::{
    CompressOptions, ConversionRequest, FrameMode, PdfOptions, PdfPage, QualityPreset,
};
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

/// A small PNG of a picture as a `data:` URL (longest side at most `size`
/// pixels, never enlarged), made like a PNG conversion (turned upright, HEIC tiles
/// joined, SVG drawn) in the private workspace: a page of the page editor.
/// `id` must not be used by another job at the same time.
pub async fn picture_thumbnail(
    bins: &Binaries,
    workspace: &Workspace,
    id: u64,
    input: &Path,
    media: &MediaInfo,
    size: u32,
) -> Result<String, CoreError> {
    if media.kind != MediaKind::Image {
        return Err(CoreError::UnsupportedConversion {
            details: "only pictures have a picture thumbnail".into(),
        });
    }
    let mut request = ConversionRequest::new(TargetFormat::Png, QualityPreset::Small);
    request.advanced.max_dimension = Some(size.clamp(16, pdf::MAX_THUMBNAIL));
    let job = ConversionJob {
        id,
        input: input.to_path_buf(),
        media: media.clone(),
        request,
        output_dir: workspace.dir().to_path_buf(),
    };
    let mut plan = plan_conversion(media, &job.request, input)?;
    if let Some(image) = plan.image.as_mut() {
        image.copy_exif = false;
    }
    let output = workspace.job_output(id, "png");
    encode(
        bins,
        workspace,
        &job,
        &plan,
        &output,
        &CancellationToken::new(),
        &mut |_| {},
    )
    .await?;
    let png = std::fs::read(&output);
    let _ = std::fs::remove_file(&output);
    Ok(format!("data:image/png;base64,{}", metadata::base64(&png?)))
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
    if let Some(options) = &plan.compress {
        return run_compress(bins, workspace, job, options, cancel, on_progress, started).await;
    }
    if plan.frames.is_some_and(|f| f.mode == FrameMode::Sequence) {
        return run_series(bins, workspace, job, &plan, cancel, on_progress, started).await;
    }
    if job.request.target == TargetFormat::Pdf && job.media.kind == MediaKind::Document {
        let inputs = [(job.input.clone(), job.media.clone())];
        return run_build(bins, workspace, job, &inputs, cancel, on_progress, started).await;
    }
    if job.request.target == TargetFormat::Docx
        || (job.request.target == TargetFormat::Pdf && job.media.kind == MediaKind::Text)
    {
        return run_document_conversion(workspace, job, cancel, on_progress, started).await;
    }
    if job.request.target == TargetFormat::Pdf {
        let inputs = [(job.input.clone(), job.media.clone())];
        return run_build(bins, workspace, job, &inputs, cancel, on_progress, started).await;
    }
    if job.media.kind == MediaKind::Document {
        return run_pages(bins, workspace, job, &plan, cancel, on_progress, started).await;
    }
    let temp_output = workspace.job_output(job.id, ext);
    encode(
        bins,
        workspace,
        job,
        &plan,
        &temp_output,
        cancel,
        &mut on_progress,
    )
    .await?;

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

/// One file made by FFmpeg (after Konvert drew an SVG picture, or prepared an
/// Ogg cover), written to `output` in the private workspace, EXIF data
/// copied. On failure nothing is left behind.
async fn encode(
    bins: &Binaries,
    workspace: &Workspace,
    job: &ConversionJob,
    plan: &ConversionPlan,
    output: &Path,
    cancel: &CancellationToken,
    on_progress: &mut (impl FnMut(Progress) + Send),
) -> Result<(), CoreError> {
    let cover_comment = prepare_cover_comment(bins, workspace, job, plan).await;
    // A vector picture is drawn first; FFmpeg then encodes the drawing.
    let drawing = match plan.image.as_ref().and_then(|i| i.render) {
        Some((width, height)) => Some(render_drawing(workspace, job, width, height).await?),
        None => None,
    };
    let source = drawing.as_deref().unwrap_or(&job.input);
    let command = build_from_plan_with(plan, source, cover_comment.as_deref(), output);

    let result = run_ffmpeg(
        bins,
        &command.args,
        job.media.duration_secs,
        cancel,
        on_progress,
    )
    .await;
    for temporary in [&cover_comment, &drawing].into_iter().flatten() {
        let _ = std::fs::remove_file(temporary);
    }
    let result = result.and_then(|()| {
        // FFmpeg can succeed without writing anything (e.g. no frame there).
        match std::fs::metadata(output) {
            Ok(m) if m.len() > 0 => Ok(()),
            _ => Err(nothing_written()),
        }
    });
    if let Err(e) = result {
        let _ = std::fs::remove_file(output);
        return Err(e);
    }

    if let Some(image) = plan
        .image
        .as_ref()
        .filter(|i| i.copy_exif || plan.remove_location)
    {
        let (input, output) = (job.input.clone(), output.to_path_buf());
        let (copy, remove, size) = (
            image.copy_exif,
            plan.remove_location,
            image.width.zip(image.height),
        );
        let _ =
            tokio::task::spawn_blocking(move || finish_exif(&input, &output, copy, remove, size))
                .await;
    }
    Ok(())
}

/// A smaller copy in the same format, named `name-compressed.ext` (with the
/// word the UI sends). A media file is converted as many times as its size
/// limit needs ([`compress::Search`]); a PDF is compressed by Konvert. The
/// copy must be smaller than the file, or nothing is written.
async fn run_compress(
    bins: &Binaries,
    workspace: &Workspace,
    job: &ConversionJob,
    options: &CompressOptions,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(Progress) + Send,
    started: Instant,
) -> Result<ConversionOutcome, CoreError> {
    let input_size = std::fs::metadata(&job.input)?.len();
    let ext = formats::spec(job.request.target).extension;
    let temp_output = workspace.job_output(job.id, ext);
    let result = if matches!(job.media.kind, MediaKind::Document | MediaKind::Text) {
        compress_document(job, options, &temp_output, cancel, &mut on_progress).await
    } else {
        compress_media(
            bins,
            workspace,
            job,
            options,
            &temp_output,
            cancel,
            &mut on_progress,
        )
        .await
    };
    let size = match result {
        Ok(size) if size < input_size => size,
        Ok(_) => {
            let _ = std::fs::remove_file(&temp_output);
            return Err(CoreError::NoGain);
        }
        Err(e) => {
            let _ = std::fs::remove_file(&temp_output);
            return Err(e);
        }
    };

    let mut stem = output_stem(&job.input);
    stem.push(format!(
        "-{}",
        label(options.label.as_deref(), "compressed")
    ));
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
    Ok(ConversionOutcome {
        output,
        size_bytes: size,
        files: 1,
        elapsed: started.elapsed(),
    })
}

/// Work that Konvert does itself (PDF and Word documents) on a worker thread,
/// with progress and cancellation: `work` gets a stop flag and a progress
/// callback, and gives the bytes of the new file.
async fn on_worker(
    work: impl FnOnce(&AtomicBool, &mut dyn FnMut(f64)) -> Result<Vec<u8>, CoreError> + Send + 'static,
    cancel: &CancellationToken,
    on_progress: &mut (impl FnMut(Progress) + Send),
) -> Result<Vec<u8>, CoreError> {
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<f64>();
    let worker = {
        let stop = stop.clone();
        tokio::task::spawn_blocking(move || {
            let mut report = |done: f64| {
                let _ = sender.send(done);
            };
            work(&stop, &mut report)
        })
    };
    let cancelled = loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break true,
            done = receiver.recv() => match done {
                Some(done) => on_progress(Progress {
                    fraction: Some(done.clamp(0.0, 1.0)),
                    ..fraction(0, 1)
                }),
                None => break false,
            },
        }
    };
    if cancelled {
        stop.store(true, Ordering::Relaxed);
        let _ = worker.await;
        return Err(CoreError::Cancelled);
    }
    worker
        .await
        .map_err(|e| CoreError::Io(std::io::Error::other(e)))?
}

/// Compresses a PDF or a Word document on a worker thread into `output`;
/// gives its size.
async fn compress_document(
    job: &ConversionJob,
    options: &CompressOptions,
    output: &Path,
    cancel: &CancellationToken,
    on_progress: &mut (impl FnMut(Progress) + Send),
) -> Result<u64, CoreError> {
    let data = std::fs::read(&job.input)?;
    let options = options.clone();
    let word = job.media.kind == MediaKind::Text;
    let bytes = on_worker(
        move |stop, progress| {
            if word {
                docx::compress::compress_docx(&data, &options, stop, progress)
            } else {
                pdf_compress::compress(&data, &options, stop, progress)
            }
        },
        cancel,
        on_progress,
    )
    .await?;
    std::fs::write(output, &bytes)?;
    Ok(bytes.len() as u64)
}

/// A Word document laid out as PDF (Typst; blocking).
fn typeset(data: &[u8]) -> Result<Vec<u8>, CoreError> {
    let package = docx::Package::open(data)?;
    let document = docx::read::read(&package)?;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        docx::typeset::to_pdf(&document)
    }))
    .unwrap_or_else(|_| {
        Err(CoreError::EngineFailed {
            details: "the layout of the document failed".into(),
        })
    })
}

/// A file made by Konvert itself: a Word document laid out as PDF, or a PDF
/// made into a Word document, next to the other outputs.
async fn run_document_conversion(
    workspace: &Workspace,
    job: &ConversionJob,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(Progress) + Send,
    started: Instant,
) -> Result<ConversionOutcome, CoreError> {
    let data = std::fs::read(&job.input)?;
    let to_word = job.request.target == TargetFormat::Docx;
    let bytes = on_worker(
        move |stop, progress| {
            if to_word {
                docx::from_pdf::pdf_to_docx(&data, stop, progress).map(|(bytes, _)| bytes)
            } else {
                progress(0.1);
                let pdf = typeset(&data)?;
                progress(1.0);
                Ok(pdf)
            }
        },
        cancel,
        &mut on_progress,
    )
    .await?;
    let ext = formats::spec(job.request.target).extension;
    let temp_output = workspace.job_output(job.id, ext);
    std::fs::write(&temp_output, &bytes)?;
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
    Ok(ConversionOutcome {
        output,
        size_bytes: bytes.len() as u64,
        files: 1,
        elapsed: started.elapsed(),
    })
}

/// Converts a media file into its own format with the settings of each
/// attempt, until one fits under the limit (or nothing is left to try): the
/// first that fits, else the smallest, ends up in `output`; gives its size.
async fn compress_media(
    bins: &Binaries,
    workspace: &Workspace,
    job: &ConversionJob,
    options: &CompressOptions,
    output: &Path,
    cancel: &CancellationToken,
    on_progress: &mut (impl FnMut(Progress) + Send),
) -> Result<u64, CoreError> {
    let ext = formats::spec(job.request.target).extension;
    let mut search = compress::Search::new(
        &job.media,
        options,
        job.request.target,
        job.request.remove_location,
    );
    let mut attempt = search.first();
    let mut kept: Option<(PathBuf, u64)> = None;
    let forget = |kept: &Option<(PathBuf, u64)>| {
        if let Some((file, _)) = kept {
            let _ = std::fs::remove_file(file);
        }
    };
    for n in 0.. {
        let plan = match attempt.plan(&job.media, &job.input) {
            Ok(plan) => plan,
            Err(e) => {
                forget(&kept);
                return Err(e);
            }
        };
        let file = workspace
            .dir()
            .join(format!("job-{}-try-{n}.{ext}", job.id));
        if let Err(e) = encode(bins, workspace, job, &plan, &file, cancel, on_progress).await {
            forget(&kept);
            return Err(e);
        }
        let size = std::fs::metadata(&file)
            .map(|m| m.len())
            .unwrap_or(u64::MAX);
        let fits = search.fits(size);
        if fits || kept.as_ref().is_none_or(|(_, best)| size < *best) {
            forget(&kept);
            kept = Some((file, size));
        } else {
            let _ = std::fs::remove_file(&file);
        }
        match search.next(&attempt, size) {
            Some(next) if !fits => attempt = next,
            _ => break,
        }
    }
    let (file, size) = kept.ok_or_else(nothing_written)?;
    if let Err(e) = std::fs::rename(&file, output) {
        let _ = std::fs::remove_file(&file);
        return Err(e.into());
    }
    Ok(size)
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

/// Several files (pictures, PDFs and Word documents) into one PDF document:
/// every page in order, or the pages of `request.pdf.layout` (whose `file`
/// is an index in `inputs`), maybe split into several documents.
#[derive(Debug, Clone)]
pub struct CombinedJob {
    pub id: u64,
    pub inputs: Vec<(PathBuf, MediaInfo)>,
    pub request: ConversionRequest,
    pub output_dir: PathBuf,
}

/// Runs a [`CombinedJob`]: one PDF named after the first file (or a folder
/// named after it, when the document is split).
pub async fn run_combined(
    bins: &Binaries,
    workspace: &Workspace,
    job: &CombinedJob,
    cancel: &CancellationToken,
    on_progress: impl FnMut(Progress) + Send,
) -> Result<ConversionOutcome, CoreError> {
    let started = Instant::now();
    let (first, media) =
        job.inputs
            .first()
            .cloned()
            .ok_or_else(|| CoreError::UnsupportedConversion {
                details: "no file to put in the document".into(),
            })?;
    if job.inputs.iter().any(|(path, _)| !path.is_file()) {
        return Err(CoreError::InputNotFound);
    }
    if !job.output_dir.is_dir() {
        return Err(CoreError::OutputUnavailable);
    }
    let request = ConversionRequest {
        target: TargetFormat::Pdf,
        ..job.request.clone()
    };
    let lead = ConversionJob {
        id: job.id,
        input: first,
        media,
        request,
        output_dir: job.output_dir.clone(),
    };
    run_build(
        bins,
        workspace,
        &lead,
        &job.inputs,
        cancel,
        on_progress,
        started,
    )
    .await
}

/// New PDFs from pictures (each encoded as JPEG by FFmpeg, turned the right
/// way up, on its own page), PDFs and Word documents (laid out by Konvert):
/// the pages of the layout (every page when there is none), in order and
/// turned, as one document or split into several. One document goes next to
/// the other outputs; several go into a new folder named after the first file
/// (`name-pages-01-03.pdf`...). The documents are built on a worker thread,
/// with cancellation between them.
async fn run_build(
    bins: &Binaries,
    workspace: &Workspace,
    job: &ConversionJob,
    inputs: &[(PathBuf, MediaInfo)],
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(Progress) + Send,
    started: Instant,
) -> Result<ConversionOutcome, CoreError> {
    let options = job.request.pdf.clone().unwrap_or_default();
    let layout = match options.layout.as_deref() {
        // The pages of a Word document exist only once it is laid out.
        Some(_) if inputs.iter().any(|(_, m)| m.kind == MediaKind::Text) => {
            return Err(CoreError::InvalidPages {
                details: "the pages of a Word document cannot be arranged one by one".into(),
            });
        }
        Some(layout) => {
            let counts: Vec<u32> = inputs
                .iter()
                .map(|(_, media)| media.document.as_ref().map_or(1, |d| d.pages))
                .collect();
            Some(pdf::check_pages(Some(layout), &counts)?)
        }
        None => None,
    };

    // Files with no page in the layout are left out; the others are read or
    // made into pages, in order.
    let used: Vec<u32> = (0..inputs.len() as u32)
        .filter(|&i| {
            layout
                .as_ref()
                .is_none_or(|l| l.iter().any(|p| p.file == i))
        })
        .collect();
    let steps = used.len() + 1;
    let picture_file = workspace.dir().join(format!("job-{}-page.jpg", job.id));
    let mut files = Vec::with_capacity(used.len());
    let result: Result<(), CoreError> =
        async {
            for (step, &i) in used.iter().enumerate() {
                let (input, media) = &inputs[i as usize];
                if cancel.is_cancelled() {
                    return Err(CoreError::Cancelled);
                }
                on_progress(fraction(step, steps));
                match media.kind {
                    MediaKind::Text => {
                        let data = std::fs::read(input)?;
                        let laid_out = tokio::task::spawn_blocking(move || typeset(&data))
                            .await
                            .map_err(|e| CoreError::Io(std::io::Error::other(e)))??;
                        files.push(pdf::Part::Document(laid_out));
                    }
                    MediaKind::Document => files.push(pdf::Part::Document(std::fs::read(input)?)),
                    _ => {
                        let request = ConversionRequest {
                            pdf: Some(PdfOptions {
                                layout: None,
                                ..options.clone()
                            }),
                            ..job.request.clone()
                        };
                        let plan = plan_conversion(media, &request, input)?;
                        let (Some(image), Some(placement)) = (
                            &plan.image,
                            plan.document.as_ref().and_then(|d| d.placement),
                        ) else {
                            return Err(CoreError::UnsupportedConversion {
                                details: "not a picture or a PDF".into(),
                            });
                        };
                        // The picture as FFmpeg makes a JPG of it (rotation, HEIC
                        // tiles, transparency on white).
                        let jpeg_plan = ConversionPlan {
                            target: TargetFormat::Jpg,
                            ..plan.clone()
                        };
                        let command = build_from_plan(&jpeg_plan, input, &picture_file);
                        run_ffmpeg(bins, &command.args, None, cancel, &mut |_| {}).await?;
                        let jpeg = std::fs::read(&picture_file)?;
                        let _ = std::fs::remove_file(&picture_file);
                        if jpeg.is_empty() {
                            return Err(nothing_written());
                        }
                        let (width, height) = image.width.zip(image.height).ok_or_else(|| {
                            CoreError::CorruptInput {
                                details: "picture size unknown".into(),
                            }
                        })?;
                        files.push(pdf::Part::Picture {
                            jpeg,
                            width,
                            height,
                            placement,
                        });
                    }
                }
            }
            Ok(())
        }
        .await;
    let _ = std::fs::remove_file(&picture_file);
    result?;
    on_progress(fraction(used.len(), steps));

    // The layout, with the files numbered among those used.
    let layout: Option<Vec<PdfPage>> = layout.map(|layout| {
        layout
            .into_iter()
            .map(|page| PdfPage {
                file: used.iter().position(|&f| f == page.file).unwrap_or(0) as u32,
                ..page
            })
            .collect()
    });
    let total = match &layout {
        Some(layout) => layout.len() as u32,
        None => files
            .iter()
            .map(|file| match file {
                pdf::Part::Picture { .. } => Ok(1),
                pdf::Part::Document(data) => pdf::page_count(data),
            })
            .sum::<Result<u32, CoreError>>()?,
    };
    let parts = pdf::split_sizes(
        options.split,
        options.split_every,
        &options.split_after,
        total,
    )?;
    // Split documents are named after their pages: the page numbers of a
    // single PDF, otherwise positions in the new document.
    let numbers: Vec<u32> = match &layout {
        Some(layout) if inputs.len() == 1 => layout.iter().map(|p| p.number).collect(),
        _ => (1..=total).collect(),
    };

    let temp_dir = workspace.job_dir(job.id);
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir)?;
    let documents = parts.len().max(1);
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<(usize, Vec<u8>)>(1);
    let builder = {
        let stop = stop.clone();
        let parts = parts.clone();
        tokio::task::spawn_blocking(move || {
            pdf::build(files, layout.as_deref(), &parts, &stop, |i, bytes| {
                sender.blocking_send((i, bytes)).is_ok()
            })
        })
    };

    let result: Result<(), CoreError> = async {
        let mut done = 0;
        loop {
            let part = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(CoreError::Cancelled),
                part = receiver.recv() => part,
            };
            let Some((i, bytes)) = part else {
                return Ok(());
            };
            std::fs::write(temp_dir.join(format!("part-{i:06}.pdf")), &bytes)?;
            done += 1;
            on_progress(fraction(used.len() * documents + done, steps * documents));
        }
    }
    .await;
    // Stop the builder (if still working) and collect its own outcome.
    stop.store(true, Ordering::Relaxed);
    drop(receiver);
    let built = builder
        .await
        .map_err(|e| CoreError::Io(std::io::Error::other(e)))?;
    let outcome = result.and(match built {
        // Stopped by us after an error or a cancellation: already reported.
        Err(CoreError::Cancelled) if cancel.is_cancelled() => Err(CoreError::Cancelled),
        other => other.map(|_| ()),
    });
    if let Err(e) = outcome {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(e);
    }

    let stem = output_stem(&job.input);
    let names = part_names(
        &stem,
        &numbers,
        &parts,
        &label(options.page_label.as_deref(), "page"),
        &label(options.pages_label.as_deref(), "pages"),
    );
    let output_dir = job.output_dir.clone();
    let temp = temp_dir.clone();
    let moved = tokio::task::spawn_blocking(move || {
        if names.is_empty() {
            let file = temp.join(format!("part-{:06}.pdf", 0));
            let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
            return naming::move_to_unique(&file, &output_dir, &stem, "pdf")
                .map(|path| (path, 1, size));
        }
        naming::move_series_named(&temp, &output_dir, &stem, "pdf", |i, _| names[i].clone())
    })
    .await
    .map_err(|e| CoreError::Io(std::io::Error::other(e)));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let (output, files, size_bytes) = moved??;
    Ok(ConversionOutcome {
        output,
        size_bytes,
        files,
        elapsed: started.elapsed(),
    })
}

/// File names of the documents of a split (empty: no split). Each is named
/// after its first and last page: the page numbers of the source when the
/// order is unchanged, otherwise positions in the new document.
fn part_names(
    stem: &OsStr,
    pages: &[u32],
    parts: &[u32],
    page_label: &str,
    pages_label: &str,
) -> Vec<OsString> {
    if parts.len() < 2 {
        return Vec::new();
    }
    let in_order = pages.is_sorted();
    let number = |position: usize| {
        if in_order {
            pages[position]
        } else {
            position as u32 + 1
        }
    };
    let digits = number(pages.len() - 1).to_string().len().max(2);
    let mut start = 0;
    parts
        .iter()
        .map(|&size| {
            let (first, last) = (number(start), number(start + size as usize - 1));
            start += size as usize;
            let mut name = stem.to_os_string();
            if size == 1 {
                name.push(format!("-{page_label}-{first:0digits$}.pdf"));
            } else {
                name.push(format!(
                    "-{pages_label}-{first:0digits$}-{last:0digits$}.pdf"
                ));
            }
            name
        })
        .collect()
}

/// Pages of a PDF as pictures, in a new folder named after the document:
/// `name-page-01.png`... The pages are drawn on a worker thread one at a time
/// (progress and cancellation between pages); PNG pages are written as they
/// are, JPG and WebP pages are encoded by FFmpeg.
async fn run_pages(
    bins: &Binaries,
    workspace: &Workspace,
    job: &ConversionJob,
    plan: &ConversionPlan,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(Progress) + Send,
    started: Instant,
) -> Result<ConversionOutcome, CoreError> {
    let document = plan
        .document
        .clone()
        .ok_or_else(|| CoreError::UnsupportedConversion {
            details: "no pages to draw".into(),
        })?;
    let ext = formats::spec(job.request.target).extension;
    let png_target = job.request.target == TargetFormat::Png;
    let temp_dir = workspace.job_dir(job.id);
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir)?;

    let data = std::fs::read(&job.input)?;
    let pages = document.pages.clone();
    let total = pages.len();
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<pdf::RenderedPage>(2);
    let renderer = {
        let stop = stop.clone();
        let dpi = document.dpi;
        tokio::task::spawn_blocking(move || {
            pdf::render_pages(data, &pages, dpi, &stop, |page| {
                sender.blocking_send(page).is_ok()
            })
        })
    };

    let result: Result<(), CoreError> = async {
        let mut done = 0;
        loop {
            let page = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(CoreError::Cancelled),
                page = receiver.recv() => page,
            };
            let Some(page) = page else { return Ok(()) };
            let name = temp_dir.join(format!("page-{:06}.{ext}", page.number));
            if png_target {
                std::fs::write(&name, &page.png)?;
            } else {
                let drawing = temp_dir.join(format!("drawing-{:06}.png", page.number));
                std::fs::write(&drawing, &page.png)?;
                let command = build_from_plan(plan, &drawing, &name);
                let encoded = run_ffmpeg(bins, &command.args, None, cancel, &mut |_| {}).await;
                let _ = std::fs::remove_file(&drawing);
                encoded?;
            }
            done += 1;
            on_progress(fraction(done, total));
        }
    }
    .await;
    // Stop the renderer (if still drawing) and collect its own outcome.
    stop.store(true, Ordering::Relaxed);
    drop(receiver);
    let rendered = renderer
        .await
        .map_err(|e| CoreError::Io(std::io::Error::other(e)))?;
    let outcome = result.and(match rendered {
        // Stopped by us after an error or a cancellation: already reported.
        Err(CoreError::Cancelled) if cancel.is_cancelled() => Err(CoreError::Cancelled),
        other => other,
    });
    if let Err(e) = outcome {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(e);
    }

    let stem = output_stem(&job.input);
    let label = label(
        job.request
            .pdf
            .as_ref()
            .and_then(|p| p.page_label.as_deref()),
        "page",
    );
    let numbers = document.pages.clone();
    let digits = document
        .pages
        .last()
        .map_or(1, |n| n.to_string().len())
        .max(2);
    let output_dir = job.output_dir.clone();
    let temp = temp_dir.clone();
    let moved = tokio::task::spawn_blocking(move || {
        // One page: a single picture next to the others, no folder.
        if let [number] = numbers[..] {
            let mut name = stem.clone();
            name.push(format!("-{label}-{number:0digits$}"));
            let file = temp.join(format!("page-{number:06}.{ext}"));
            let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
            return naming::move_to_unique(&file, &output_dir, &name, ext)
                .map(|path| (path, 1, size));
        }
        naming::move_series_named(&temp, &output_dir, &stem, ext, |i, _| {
            let mut name = stem.clone();
            name.push(format!("-{label}-{:0digits$}.{ext}", numbers[i]));
            name
        })
    })
    .await
    .map_err(|e| CoreError::Io(std::io::Error::other(e)));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let (output, files, size_bytes) = moved??;
    Ok(ConversionOutcome {
        output,
        size_bytes,
        files,
        elapsed: started.elapsed(),
    })
}

/// Word between the name and page numbers in file names: letters only (the
/// UI sends "pagina" or "page"), `fallback` otherwise.
fn label(label: Option<&str>, fallback: &str) -> String {
    let clean: String = label
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_alphabetic())
        .take(20)
        .collect();
    if clean.is_empty() {
        fallback.into()
    } else {
        clean
    }
}

fn fraction(done: usize, total: usize) -> Progress {
    Progress {
        fraction: Some(done as f64 / total.max(1) as f64),
        eta_secs: None,
        speed: None,
        processed_secs: None,
        output_bytes: None,
    }
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

    #[test]
    fn split_documents_are_named_after_their_pages() {
        let names = |pages: &[u32], parts: &[u32]| -> Vec<String> {
            part_names(OsStr::new("Atto"), pages, parts, "pagina", "pagine")
                .into_iter()
                .map(|n| n.into_string().unwrap())
                .collect()
        };
        assert!(names(&[1, 2, 3], &[]).is_empty());
        assert_eq!(
            names(&[1, 2, 3, 5], &[3, 1]),
            ["Atto-pagine-01-03.pdf", "Atto-pagina-05.pdf"]
        );
        // A new order: positions in the new document.
        assert_eq!(
            names(&[3, 1, 2], &[1, 2]),
            ["Atto-pagina-01.pdf", "Atto-pagine-02-03.pdf"]
        );
        let long: Vec<u32> = (1..=120).collect();
        assert_eq!(names(&long, &[100, 20])[1], "Atto-pagine-101-120.pdf");
        assert_eq!(label(Some("pa/gi..na"), "page"), "pagina");
        assert_eq!(label(None, "pages"), "pages");
    }
}
