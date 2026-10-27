// State machine of the conversion queue (one or more files):
// empty → setup (analysis, choices) → converting → finished (and back).
// Pure reducer and selectors; side effects live in `useQueue`.
import type { AdvancedOptions } from "../bindings/AdvancedOptions";
import type { CompressOptions } from "../bindings/CompressOptions";
import type { ConversionOutcome } from "../bindings/ConversionOutcome";
import type { ConversionPlan } from "../bindings/ConversionPlan";
import type { ConversionRequest } from "../bindings/ConversionRequest";
import type { ConversionWarning } from "../bindings/ConversionWarning";
import type { ErrorPayload } from "../bindings/ErrorPayload";
import type { FileAnalysis } from "../bindings/FileAnalysis";
import type { FrameOptions } from "../bindings/FrameOptions";
import type { JobEvent } from "../bindings/JobEvent";
import type { PdfOptions } from "../bindings/PdfOptions";
import type { Progress } from "../bindings/Progress";
import type { QualityPreset } from "../bindings/QualityPreset";
import type { TargetFormat } from "../bindings/TargetFormat";
import type { TargetOption } from "../bindings/TargetOption";
import { cutPositions, isEdited, keptCount, layoutOf, type EditorPage } from "./pages";

export type ItemStatus =
  | "analyzing"
  | "ready"
  | "invalid"
  | "queued"
  | "running"
  | "done"
  | "failed"
  | "cancelled";

export interface QueueItem {
  /** Local identifier (stable while the item is in the list). */
  key: number;
  path: string;
  name: string;
  status: ItemStatus;
  file: FileAnalysis | null;
  /** Analysis or conversion error. */
  error: ErrorPayload | null;
  /** Plan for the current request (`null` while computing). */
  plan: ConversionPlan | null;
  planError: ErrorPayload | null;
  jobId: number | null;
  progress: Progress | null;
  outcome: ConversionOutcome | null;
}

export interface QueueState {
  phase: "empty" | "setup" | "converting" | "finished";
  items: QueueItem[];
  target: TargetFormat | null;
  /** A smaller copy of every file, each in its own format (instead of a target format). */
  compress: boolean;
  /** Level, size limit and PDF pages as pictures, for smaller copies. */
  compression: CompressOptions;
  preset: QualityPreset;
  advanced: AdvancedOptions;
  /** Pictures taken from videos (frame targets only). */
  frames: FrameOptions;
  /** Erase where the files were made (GPS data, location tags). */
  removeLocation: boolean;
  /** Pictures to PDF and PDF to pictures. */
  pdf: PdfOptions;
  /** A single PDF made into a new one: its pages as edited (`null`: untouched). */
  pages: EditorPage[] | null;
  /** Page numbers after which a split starts a new document (split "after"). */
  cuts: number[];
  /** The running or finished conversion made one PDF out of every file. */
  combined: boolean;
  /** Bumped whenever the request changes: answers for older requests are discarded. */
  planVersion: number;
  notice: "cancelled" | null;
  cancelling: boolean;
  nextKey: number;
}

export type QueueAction =
  | { type: "add"; paths: string[] }
  | { type: "analyzed"; key: number; file: FileAnalysis }
  | { type: "analyzeFailed"; key: number; error: ErrorPayload }
  | { type: "remove"; key: number }
  | { type: "move"; key: number; delta: -1 | 1 }
  | { type: "selectTarget"; target: TargetFormat }
  | { type: "selectCompress" }
  | { type: "setCompression"; compression: CompressOptions }
  | { type: "selectPreset"; preset: QualityPreset }
  | { type: "setAdvanced"; advanced: AdvancedOptions }
  | { type: "setFrames"; frames: FrameOptions }
  | { type: "setRemoveLocation"; removeLocation: boolean }
  | { type: "setPdf"; pdf: PdfOptions }
  | { type: "setPages"; pages: EditorPage[] | null; cuts: number[] }
  | { type: "defaultPreset"; preset: QualityPreset }
  | { type: "planned"; key: number; version: number; plan: ConversionPlan }
  | { type: "planFailed"; key: number; version: number; error: ErrorPayload }
  | { type: "start" }
  | { type: "started"; key: number; jobId: number }
  | { type: "startFailed"; key: number; error: ErrorPayload }
  | { type: "job"; event: JobEvent }
  | { type: "cancelAll" }
  | { type: "back" }
  | { type: "reset" };

export const NO_TARGETS = "no_targets";

/** One frame, in the middle of the video. */
export const DEFAULT_FRAMES: FrameOptions = { mode: "single", atSecs: null, everySecs: null };

/**
 * A4 pages chosen by orientation, small margins, one PDF; 150 dpi pictures of
 * every page; an edited PDF is not split.
 */
export const DEFAULT_PDF: PdfOptions = {
  pageSize: "a4",
  orientation: "auto",
  margin: "small",
  combine: true,
  dpi: 150,
  pages: null,
  pageLabel: null,
  pagesLabel: null,
  layout: null,
  split: "none",
  splitEvery: 1,
  splitAfter: [],
};

/** Strong compression, no size limit, text kept as text. */
export const DEFAULT_COMPRESSION: CompressOptions = { level: "strong", maxBytes: null, pagesAsPictures: false, label: null };

export function initialQueue(preset: QualityPreset = "balanced"): QueueState {
  return {
    phase: "empty",
    items: [],
    target: null,
    compress: false,
    compression: DEFAULT_COMPRESSION,
    preset,
    advanced: {},
    frames: DEFAULT_FRAMES,
    removeLocation: false,
    pdf: DEFAULT_PDF,
    pages: null,
    cuts: [],
    combined: false,
    planVersion: 0,
    notice: null,
    cancelling: false,
    nextKey: 1,
  };
}

const CONVERTING: ItemStatus[] = ["queued", "running", "done", "failed", "cancelled"];
const FINISHED: ItemStatus[] = ["done", "failed", "cancelled"];

const basename = (path: string) => path.split(/[\\/]/).pop() ?? path;

// --- Selectors ---------------------------------------------------------------------

/** Items that take part in the conversion (analysed and convertible). */
export function readyItems(state: QueueState): QueueItem[] {
  return state.items.filter((i) => i.status === "ready" && i.file);
}

/** Same kind of conversion: "audio" and "audio only" are both audio files. */
const sameGroup = (a: TargetOption["group"], b: TargetOption["group"]) =>
  a === b || (["audio", "audioOnly"].includes(a) && ["audio", "audioOnly"].includes(b));

/**
 * Target formats offered for every analysed file (intersection). A format must
 * mean the same thing for every file: JPG is a converted photo for a picture
 * but a frame for a video, so photos and videos share no picture format.
 */
export function commonTargets(items: QueueItem[]): TargetOption[] {
  const files = items.filter((i) => i.file && i.status !== "invalid").map((i) => i.file!);
  if (files.length === 0) return [];
  const mixedKinds = new Set(files.map((f) => f.media.kind === "audio")).size > 1;
  return files[0].targets
    .filter((option) =>
      files.every((f) => f.targets.some((o) => o.format === option.format && sameGroup(o.group, option.group))),
    )
    .map((option) => ({
      ...option,
      // Audio files and videos together: the audio formats are just "audio".
      group: mixedKinds && option.group === "audioOnly" ? "audio" : option.group,
      sameAsInput: files.every((f) => f.targets.find((o) => o.format === option.format)?.sameAsInput),
    }));
}

/** Every analysed file can be made smaller in its own format. */
export function canCompress(items: QueueItem[]): boolean {
  const files = items.filter((i) => i.file && i.status !== "invalid");
  return files.length > 0 && files.every((i) => i.file!.compress != null);
}

/** The only file is a PDF: as a PDF target, its pages can be edited and split. */
export function lonePdf(items: QueueItem[]): QueueItem | null {
  const files = items.filter((i) => i.file && i.status !== "invalid");
  return files.length === 1 && files[0].file!.media.kind === "document" ? files[0] : null;
}

/** A single PDF made into one or more new PDFs (page editor and split). */
export function editsPdf(state: QueueState): boolean {
  return state.target === "pdf" && lonePdf(state.items) != null;
}

export function requestOf(state: QueueState): ConversionRequest | null {
  if (!state.target) return null;
  const edit = editsPdf(state);
  const count = lonePdf(state.items)?.file?.media.document?.pages ?? 0;
  return {
    target: state.target,
    preset: state.preset,
    advanced: state.advanced,
    frames: state.frames,
    removeLocation: state.removeLocation,
    // Page edits and splits belong to a single PDF: never to merged files.
    pdf: edit
      ? { ...state.pdf, layout: layoutOf(state.pages), splitAfter: cutPositions(state.pages, state.cuts, count) }
      : { ...state.pdf, layout: null, split: "none", splitAfter: [] },
  };
}

/** The request for one file: the shared choices, or a smaller copy in the file's own format. */
export function requestFor(state: QueueState, item: QueueItem): ConversionRequest | null {
  if (!state.compress) return requestOf(state);
  const target = item.file?.compress;
  if (!target) return null;
  return {
    target,
    preset: state.preset,
    advanced: {},
    frames: null,
    removeLocation: state.removeLocation,
    pdf: null,
    compress: state.compression,
  };
}

/** Documents an edited PDF becomes, by the same rules as the core. */
export function editedDocuments(state: QueueState): number {
  const count = lonePdf(state.items)?.file?.media.document?.pages ?? 0;
  const kept = state.pages ? keptCount(state.pages) : count;
  if (state.pdf.split === "every") return Math.max(1, Math.ceil(kept / Math.max(1, state.pdf.splitEvery)));
  if (state.pdf.split === "after") return cutPositions(state.pages, state.cuts, count).length + 1;
  return 1;
}

/** The edited PDF differs from the original: pages changed, or several documents. */
export function pdfChanged(state: QueueState): boolean {
  return isEdited(state.pages) || editedDocuments(state) > 1;
}

/** Every file goes into one PDF: always for PDFs, for pictures unless turned off. */
export function combines(state: QueueState): boolean {
  const ready = readyItems(state);
  return (
    state.target === "pdf" &&
    ready.length > 1 &&
    (state.pdf.combine || ready.some((i) => i.file!.media.kind === "document"))
  );
}

/** Items shown as results: a combined PDF is one result, shared by every file. */
export function resultItems(state: QueueState): QueueItem[] {
  const finished = state.items.filter((i) => FINISHED.includes(i.status));
  return state.combined ? finished.slice(0, 1) : finished;
}

export function canStart(state: QueueState): boolean {
  const ready = readyItems(state);
  return (
    state.phase === "setup" &&
    (state.target != null || (state.compress && canCompress(ready))) &&
    ready.length > 0 &&
    !state.items.some((i) => i.status === "analyzing") &&
    ready.every((i) => i.plan && !i.planError) &&
    // A copy of a PDF with nothing changed would be pointless.
    (!editsPdf(state) || pdfChanged(state))
  );
}

/** Sum of the estimates; `null` if any is unknown. */
export function totalEstimate(items: QueueItem[]): { bytes: number; approximate: boolean } | null {
  let bytes = 0;
  let approximate = false;
  for (const item of items) {
    const e = item.plan?.estimate;
    if (!e) return null;
    bytes += e.bytes;
    approximate ||= e.approximate;
  }
  return items.length > 0 ? { bytes, approximate } : null;
}

/** Union of the warnings of every item, in importance order. */
export function allWarnings(items: QueueItem[]): ConversionWarning[] {
  const seen = new Set<ConversionWarning>();
  for (const item of items) item.plan?.warnings.forEach((w) => seen.add(w));
  return [...seen];
}

/** Overall completion between 0 and 1, weighted by duration. */
export function overallProgress(state: QueueState): number {
  const items = state.items.filter((i) => CONVERTING.includes(i.status));
  if (items.length === 0) return 0;
  const durations = items.map((i) => i.file?.media.durationSecs ?? null);
  const known = durations.filter((d): d is number => d != null && d > 0);
  const fallback = known.length ? known.reduce((a, b) => a + b, 0) / known.length : 1;
  let total = 0;
  let done = 0;
  items.forEach((item, index) => {
    const weight = durations[index] ?? fallback;
    const fraction = FINISHED.includes(item.status) ? 1 : item.status === "running" ? (item.progress?.fraction ?? 0) : 0;
    total += weight;
    done += weight * fraction;
  });
  return total > 0 ? done / total : 0;
}

export function counts(state: QueueState) {
  const of = (s: ItemStatus) => state.items.filter((i) => i.status === s).length;
  const total = state.items.filter((i) => CONVERTING.includes(i.status)).length;
  return { done: of("done"), failed: of("failed"), cancelled: of("cancelled"), finished: of("done") + of("failed") + of("cancelled"), total };
}

// --- Reducer -----------------------------------------------------------------------

function update(state: QueueState, key: number, patch: Partial<QueueItem>): QueueState {
  return { ...state, items: state.items.map((i) => (i.key === key ? { ...i, ...patch } : i)) };
}

/** Drops every plan (the request changed). */
function invalidate(state: QueueState): QueueState {
  return {
    ...state,
    planVersion: state.planVersion + 1,
    items: state.items.map((i) => ({ ...i, plan: null, planError: null })),
  };
}

/** After the file list changes: keep the target only if still offered for every file. */
function revalidateTarget(state: QueueState): QueueState {
  if (state.target && !commonTargets(state.items).some((o) => o.format === state.target)) {
    return invalidate({ ...state, target: null, advanced: {} });
  }
  if (state.compress && !canCompress(state.items)) {
    return invalidate({ ...state, compress: false });
  }
  return state;
}

function withoutItems(state: QueueState, items: QueueItem[]): QueueState {
  if (items.length === 0) return { ...initialQueue(state.preset), nextKey: state.nextKey };
  return revalidateTarget({ ...state, items });
}

/** Ends the conversion phase once every item is finished. */
function maybeFinish(state: QueueState): QueueState {
  const active = state.items.filter((i) => CONVERTING.includes(i.status));
  if (active.some((i) => !FINISHED.includes(i.status))) return state;
  if (active.every((i) => i.status === "cancelled")) {
    // Nothing was produced: back to the choices, ready to try again.
    return {
      ...state,
      phase: "setup",
      cancelling: false,
      combined: false,
      notice: "cancelled",
      items: state.items.map((i) => (i.status === "cancelled" ? { ...i, status: "ready", jobId: null, progress: null } : i)),
    };
  }
  return { ...state, phase: "finished", cancelling: false };
}

export function queueReducer(state: QueueState, action: QueueAction): QueueState {
  switch (action.type) {
    case "add": {
      if (state.phase === "converting") return state;
      const base = state.phase === "finished" ? { ...initialQueue(state.preset), nextKey: state.nextKey } : state;
      const known = new Set(base.items.map((i) => i.path));
      const fresh = [...new Set(action.paths)].filter((p) => !known.has(p));
      if (fresh.length === 0) return base;
      let nextKey = base.nextKey;
      const added: QueueItem[] = fresh.map((path) => ({
        key: nextKey++,
        path,
        name: basename(path),
        status: "analyzing",
        file: null,
        error: null,
        plan: null,
        planError: null,
        jobId: null,
        progress: null,
        outcome: null,
      }));
      return { ...base, phase: "setup", notice: null, nextKey, items: [...base.items, ...added] };
    }

    case "analyzed": {
      const item = state.items.find((i) => i.key === action.key);
      if (!item || item.status !== "analyzing") return state;
      const usable = action.file.targets.length > 0;
      const next = update(state, action.key, {
        file: action.file,
        name: action.file.name,
        status: usable ? "ready" : "invalid",
        error: usable ? null : { code: NO_TARGETS, details: null },
      });
      return revalidateTarget(next);
    }

    case "analyzeFailed": {
      const item = state.items.find((i) => i.key === action.key);
      if (!item || item.status !== "analyzing") return state;
      return update(state, action.key, { status: "invalid", error: action.error });
    }

    case "remove": {
      if (state.phase !== "setup") return state;
      const removed = state.items.find((i) => i.key === action.key);
      const next = withoutItems(state, state.items.filter((i) => i.key !== action.key));
      // Page edits belong to the PDF they were made on.
      return removed?.file?.media.kind === "document" && next.pages
        ? invalidate({ ...next, pages: null, cuts: [] })
        : next;
    }

    case "move": {
      // Order of the pages in a combined PDF.
      if (state.phase !== "setup") return state;
      const from = state.items.findIndex((i) => i.key === action.key);
      const to = from + action.delta;
      if (from < 0 || to < 0 || to >= state.items.length) return state;
      const items = [...state.items];
      [items[from], items[to]] = [items[to], items[from]];
      return { ...state, items };
    }

    case "selectTarget":
      if (state.phase !== "setup" || (state.target === action.target && !state.compress)) return state;
      // Codec choices of another format may not apply: start from automatic.
      return invalidate({ ...state, target: action.target, compress: false, advanced: {}, notice: null });

    case "selectCompress":
      if (state.phase !== "setup" || state.compress || !canCompress(state.items)) return state;
      return invalidate({ ...state, compress: true, target: null, advanced: {}, notice: null });

    case "setCompression":
      if (state.phase !== "setup") return state;
      return invalidate({ ...state, compression: action.compression, notice: null });

    case "selectPreset":
      if (state.phase !== "setup" || state.preset === action.preset) return state;
      return invalidate({ ...state, preset: action.preset, notice: null });

    case "setAdvanced":
      if (state.phase !== "setup") return state;
      return invalidate({ ...state, advanced: action.advanced, notice: null });

    case "setFrames":
      if (state.phase !== "setup") return state;
      return invalidate({ ...state, frames: action.frames, notice: null });

    case "setRemoveLocation":
      if (state.phase !== "setup") return state;
      return invalidate({ ...state, removeLocation: action.removeLocation, notice: null });

    case "setPdf":
      if (state.phase !== "setup") return state;
      return invalidate({ ...state, pdf: action.pdf, notice: null });

    case "setPages":
      if (state.phase !== "setup") return state;
      return invalidate({ ...state, pages: action.pages, cuts: action.cuts, notice: null });

    case "defaultPreset":
      return state.phase === "empty" ? { ...state, preset: action.preset } : state;

    case "planned":
    case "planFailed": {
      if (action.version !== state.planVersion) return state;
      return action.type === "planned"
        ? update(state, action.key, { plan: action.plan, planError: null })
        : update(state, action.key, { plan: null, planError: action.error });
    }

    case "start":
      if (!canStart(state)) return state;
      return {
        ...state,
        phase: "converting",
        notice: null,
        cancelling: false,
        combined: combines(state),
        items: state.items.map((i) =>
          i.status === "ready" ? { ...i, status: "queued", jobId: null, progress: null, outcome: null, error: null } : i,
        ),
      };

    case "started":
      if (state.phase !== "converting") return state;
      return update(state, action.key, { jobId: action.jobId });

    case "startFailed":
      if (state.phase !== "converting") return state;
      return maybeFinish(update(state, action.key, { status: "failed", error: action.error }));

    case "job": {
      if (state.phase !== "converting") return state;
      // Every file of a combined PDF shares the same job.
      const jobItems = state.items.filter((i) => i.jobId === action.event.jobId && !FINISHED.includes(i.status));
      if (jobItems.length === 0) return state;
      const u = action.event.update;
      const patch: Partial<QueueItem> | null =
        u.state === "running"
          ? { status: "running" }
          : u.state === "progress"
            ? { status: "running", progress: u.progress }
            : u.state === "done"
              ? { status: "done", outcome: u.outcome }
              : u.state === "failed"
                ? { status: "failed", error: u.error }
                : u.state === "cancelled"
                  ? { status: "cancelled" }
                  : null;
      if (!patch) return state;
      const keys = new Set(jobItems.map((i) => i.key));
      const next = { ...state, items: state.items.map((i) => (keys.has(i.key) ? { ...i, ...patch } : i)) };
      return FINISHED.includes(patch.status!) ? maybeFinish(next) : next;
    }

    case "cancelAll": {
      if (state.phase !== "converting") return state;
      // Items not handed to the backend yet will never start: cancel them now.
      const items = state.items.map((i): QueueItem =>
        i.status === "queued" && i.jobId == null ? { ...i, status: "cancelled" } : i,
      );
      return maybeFinish({ ...state, cancelling: true, items });
    }

    case "back":
      // From a finished conversion back to the choices (e.g. retry after an error).
      if (state.phase !== "finished") return state;
      return {
        ...state,
        phase: "setup",
        combined: false,
        items: state.items.map((i) =>
          FINISHED.includes(i.status) ? { ...i, status: "ready", jobId: null, progress: null, outcome: null, error: null } : i,
        ),
      };

    case "reset":
      return state.phase === "converting" ? state : { ...initialQueue(state.preset), nextKey: state.nextKey };
  }
}
