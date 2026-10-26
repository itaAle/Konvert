//! Conversion queue.
//!
//! Jobs run one at a time (FFmpeg already uses every CPU core) on the async
//! runtime, so the UI never blocks. The UI follows each job through
//! [`JOB_EVENT`] events. Output paths are kept in memory only, so that "open
//! file" / "show in folder" can act on files produced in this session; nothing
//! is written to disk (except the optional, redacted debug log).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use konvert_core::formats;
use konvert_core::plan::{AudioPlan, VideoPlan};
use konvert_core::{
    Binaries, ConversionJob, ConversionOutcome, ConversionRequest, CoreError, ErrorPayload,
    MediaInfo, Progress, Workspace, plan_conversion, probe, run_conversion,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::debug_log::{DebugLog, Redactor};

/// Name of the event carrying [`JobEvent`]s.
pub const JOB_EVENT: &str = "job-update";

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JobEvent {
    pub job_id: u64,
    pub update: JobUpdate,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
#[ts(export)]
pub enum JobUpdate {
    /// Waiting for the previous job to finish.
    Queued,
    Running,
    Progress {
        progress: Progress,
    },
    Done {
        outcome: ConversionOutcome,
    },
    Failed {
        error: ErrorPayload,
    },
    Cancelled,
}

pub struct JobQueue {
    log: Arc<DebugLog>,
    next_id: AtomicU64,
    /// Held by the running job: serializes conversions.
    turn: tokio::sync::Mutex<()>,
    cancels: Mutex<HashMap<u64, CancellationToken>>,
    outputs: Mutex<HashMap<u64, PathBuf>>,
}

impl JobQueue {
    pub fn new(log: Arc<DebugLog>) -> Self {
        Self {
            log,
            next_id: AtomicU64::new(0),
            turn: tokio::sync::Mutex::new(()),
            cancels: Mutex::default(),
            outputs: Mutex::default(),
        }
    }

    /// Queues a conversion and returns its id immediately.
    pub fn enqueue(
        self: &Arc<Self>,
        app: AppHandle,
        bins: Binaries,
        workspace: Arc<Workspace>,
        input: PathBuf,
        request: ConversionRequest,
        output_dir: PathBuf,
    ) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let token = CancellationToken::new();
        lock(&self.cancels).insert(id, token.clone());
        emit(&app, id, JobUpdate::Queued);
        self.log.write(&format!(
            "job {id}: queued, target {:?}, preset {:?}, advanced {:?}",
            request.target, request.preset, request.advanced
        ));

        let queue = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let job = Job {
                id,
                input,
                request,
                output_dir,
            };
            let redactor = Redactor::new()
                .path(&job.input, "input")
                .path(&job.output_dir, "output-dir")
                .path(
                    &workspace.job_output(id, formats::spec(job.request.target).extension),
                    "temp",
                )
                .path(&workspace.job_dir(id), "temp-dir");
            let update = queue.run(&app, &bins, &workspace, job, &token).await;
            lock(&queue.cancels).remove(&id);
            queue.log_outcome(id, &update, &redactor);
            emit(&app, id, update);
        });
        id
    }

    async fn run(
        &self,
        app: &AppHandle,
        bins: &Binaries,
        workspace: &Workspace,
        job: Job,
        token: &CancellationToken,
    ) -> JobUpdate {
        let _turn = tokio::select! {
            turn = self.turn.lock() => turn,
            _ = token.cancelled() => return JobUpdate::Cancelled,
        };
        emit(app, job.id, JobUpdate::Running);

        // Analyse again from disk: the UI only sends a path and a request.
        let media = match probe(bins, &job.input).await {
            Ok(media) => media,
            Err(e) => return failed(e),
        };
        self.log_start(job.id, &media, &job);
        let conversion = ConversionJob {
            id: job.id,
            input: job.input,
            media,
            request: job.request,
            output_dir: job.output_dir,
        };
        let on_progress = |progress| emit(app, job.id, JobUpdate::Progress { progress });
        match run_conversion(bins, workspace, &conversion, token, on_progress).await {
            Ok(outcome) => {
                lock(&self.outputs).insert(job.id, outcome.output.clone());
                JobUpdate::Done { outcome }
            }
            Err(CoreError::Cancelled) => JobUpdate::Cancelled,
            Err(e) => failed(e),
        }
    }

    fn log_start(&self, id: u64, media: &MediaInfo, job: &Job) {
        if !self.log.is_enabled() {
            return;
        }
        let mut line = format!(
            "job {id}: input {:?} in {}, {:.1} s",
            media.kind,
            media.container,
            media.duration_secs.unwrap_or(0.0)
        );
        if let Ok(plan) = plan_conversion(media, &job.request, &job.input) {
            let video = match &plan.video {
                Some(VideoPlan::Copy { codec }) => format!("copy {codec}"),
                Some(VideoPlan::Encode(e)) => format!("encode {:?} {:?}", e.codec, e.rate),
                None => "none".into(),
            };
            let audio = match &plan.audio {
                Some(AudioPlan::Copy { codec }) => format!("copy {codec}"),
                Some(AudioPlan::Encode(e)) => format!("encode {:?} {:?}", e.codec, e.rate),
                None => "none".into(),
            };
            let image = match &plan.image {
                Some(i) => format!(
                    "encode {:?} {:?} {}x{}{}",
                    i.codec,
                    i.quality,
                    i.width.unwrap_or(0),
                    i.height.unwrap_or(0),
                    if i.flatten { " on white" } else { "" }
                ),
                None => "none".into(),
            };
            line.push_str(&format!(
                "; video {video}; audio {audio}; picture {image}; frames {:?}; warnings {:?}",
                plan.frames, plan.warnings
            ));
        }
        self.log.write(&line);
    }

    fn log_outcome(&self, id: u64, update: &JobUpdate, redactor: &Redactor) {
        if !self.log.is_enabled() {
            return;
        }
        let line = match update {
            JobUpdate::Done { outcome } => format!(
                "job {id}: done in {:.1} s, {} file(s), {} bytes",
                outcome.elapsed.as_secs_f64(),
                outcome.files,
                outcome.size_bytes
            ),
            JobUpdate::Cancelled => format!("job {id}: cancelled"),
            JobUpdate::Failed { error } => format!(
                "job {id}: failed [{}]
{}",
                error.code,
                redactor.apply(error.details.as_deref().unwrap_or(""))
            ),
            _ => return,
        };
        self.log.write(&line);
    }

    /// At least one job is queued or running.
    pub fn has_active(&self) -> bool {
        !lock(&self.cancels).is_empty()
    }

    /// Requests cancellation; `false` if the job is unknown or already finished.
    pub fn cancel(&self, id: u64) -> bool {
        match lock(&self.cancels).get(&id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// Cancels every queued or running job (application exit).
    pub fn cancel_all(&self) {
        for token in lock(&self.cancels).values() {
            token.cancel();
        }
    }

    /// Output of a finished job of this session.
    pub fn output(&self, id: u64) -> Option<PathBuf> {
        lock(&self.outputs).get(&id).cloned()
    }
}

struct Job {
    id: u64,
    input: PathBuf,
    request: ConversionRequest,
    output_dir: PathBuf,
}

fn failed(e: CoreError) -> JobUpdate {
    JobUpdate::Failed { error: e.payload() }
}

fn emit(app: &AppHandle, job_id: u64, update: JobUpdate) {
    let _ = app.emit(JOB_EVENT, JobEvent { job_id, update });
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
