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
import type { AudioFile } from "../bindings/AudioFile";
import type { Volume } from "../bindings/Volume";
import type { ExtractTrack } from "../bindings/ExtractTrack";
import type { MediaEdit } from "../bindings/MediaEdit";
import type { TrackChoice } from "../bindings/TrackChoice";
import { DEFAULT_EDIT, editChanged, pictureEditOf, type EditScope, type EditState } from "./edit";
import { defaultTracks, sameTracks } from "./tracks";
import { isTrimmed, type TrimState } from "./trim";
import { cutPositions, isEdited, keptCount, layoutOf, moveFile, reconcile, type EditorPage, type PageSource } from "./pages";

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
  /** An edited copy of every picture, each in its own format ("Modifica"). */
  editCopy: boolean;
  /** Turn, mirroring, crop and new size of pictures. */
  edit: EditState;
  /** Tracks kept from the only audio or video file, in order (`null`: the default ones). */
  tracks: TrackChoice[] | null;
  /** The part kept of the only audio or video file (`null`: all of it). */
  trim: TrimState | null;
  /** The audio of another file added to the only video, or replacing its audio. */
  audioFile: AudioFile | null;
  /** Several audio or video files joined into one, in the order of the list. */
  join: boolean;
  /** The loudness of the audio of the only file (`null`: as it is). */
  volume: Volume | null;
  /** Tracks of the only audio or video file saved as files of their own (instead of a target format). */
  extract: boolean;
  /** The tracks chosen for that, in file order. */
  extractTracks: ExtractTrack[];
  preset: QualityPreset;
  advanced: AdvancedOptions;
  /** Pictures taken from videos (frame targets only). */
  frames: FrameOptions;
  /** Erase where the files were made (GPS data, location tags). */
  removeLocation: boolean;
  /** Pictures to PDF and PDF to pictures. */
  pdf: PdfOptions;
  /** The pages of the PDF being made (a lone PDF, or files combined), as
   * edited (`null`: untouched). */
  pages: EditorPage[] | null;
  /** Pages (their `pageId`) after which a split starts a new document (split "after"). */
  cuts: string[];
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
  /** A PDF that needed a password was opened with it. */
  | { type: "unlocked"; key: number; file: FileAnalysis }
  | { type: "remove"; key: number }
  | { type: "move"; key: number; delta: -1 | 1 }
  | { type: "selectTarget"; target: TargetFormat }
  | { type: "selectCompress" }
  | { type: "setCompression"; compression: CompressOptions }
  | { type: "selectEditCopy" }
  | { type: "setEdit"; edit: EditState }
  | { type: "setTracks"; tracks: TrackChoice[] | null }
  | { type: "setTrim"; trim: TrimState | null }
  | { type: "setAudioFile"; audioFile: AudioFile | null }
  | { type: "setVolume"; volume: Volume | null }
  | { type: "setJoin"; join: boolean }
  | { type: "selectExtract" }
  | { type: "setExtract"; tracks: ExtractTrack[] }
  | { type: "selectPreset"; preset: QualityPreset }
  | { type: "setAdvanced"; advanced: AdvancedOptions }
  | { type: "setFrames"; frames: FrameOptions }
  | { type: "setRemoveLocation"; removeLocation: boolean }
  | { type: "setPdf"; pdf: PdfOptions }
  | { type: "setPages"; pages: EditorPage[] | null; cuts: string[] }
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
  protection: null,
  watermark: null,
  pageNumbers: null,
  ocr: false,
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
    editCopy: false,
    edit: DEFAULT_EDIT,
    tracks: null,
    trim: null,
    audioFile: null,
    volume: null,
    join: false,
    extract: false,
    extractTracks: [],
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

/** Word after the name of an edited copy (`name-mod.jpg`). */
export const EDIT_LABEL = "mod";

/** Every analysed file is a picture Konvert can write in its own format. */
export function canEditCopy(items: QueueItem[]): boolean {
  const files = items.filter((i) => i.file && i.status !== "invalid");
  return files.length > 0 && files.every((i) => i.file!.edit != null);
}

/** One photo only: it can be cropped. */
function lonePhoto(state: QueueState): boolean {
  const ready = readyItems(state);
  return ready.length === 1 && state.items.length === 1 && ready[0].file!.media.kind === "image";
}

/**
 * What can be changed in the pictures being made: photos into pictures or
 * edited copies (turned, mirrored, cropped when alone, resized except icons),
 * pictures taken from videos (resized). `null`: no picture edits.
 */
export function editScope(state: QueueState): EditScope | null {
  if (state.compress) return null;
  if (state.editCopy) {
    if (!readyItems(state).some((i) => i.file!.media.kind === "image")) return null;
    const icons = readyItems(state).every((i) => i.file!.edit === "ico");
    return { turn: true, crop: lonePhoto(state), resize: !icons };
  }
  const option = commonTargets(state.items).find((o) => o.format === state.target);
  if (option?.group === "image") return { turn: true, crop: lonePhoto(state), resize: option.format !== "ico" };
  if (option?.group === "frames") return { turn: false, crop: false, resize: true };
  return null;
}

const MEDIA_KINDS = ["audio", "video", "videoWithAudio"];

/**
 * The only file is audio or video, made into audio or video (or an edited
 * copy): its tracks can be chosen. `audio`: only its audio tracks matter.
 */
export function trackScope(state: QueueState): { file: FileAnalysis; audio: boolean } | null {
  const ready = readyItems(state);
  if (state.compress || ready.length !== 1 || state.items.length !== 1) return null;
  const file = ready[0].file!;
  if (!MEDIA_KINDS.includes(file.media.kind) || file.media.tracks.length === 0) return null;
  if (state.editCopy) return { file, audio: file.media.kind === "audio" };
  const option = commonTargets(state.items).find((o) => o.format === state.target);
  if (!option || !["video", "audio", "audioOnly"].includes(option.group)) return null;
  // Animations have no audio or subtitles.
  if (ANIMATIONS.includes(option.format)) return null;
  return { file, audio: option.group !== "video" };
}

/**
 * The only file is audio or video with a known length, made into audio or
 * video (animations too) or an edited copy: a part of it can be kept.
 */
export function trimScope(state: QueueState): { file: FileAnalysis; duration: number; video: boolean } | null {
  const ready = readyItems(state);
  if (state.compress || ready.length !== 1 || state.items.length !== 1) return null;
  const file = ready[0].file!;
  const duration = file.media.durationSecs;
  if (!MEDIA_KINDS.includes(file.media.kind) || !duration) return null;
  const video = file.media.kind !== "audio";
  if (state.editCopy || state.extract) return { file, duration, video };
  const option = commonTargets(state.items).find((o) => o.format === state.target);
  if (!option || !["video", "audio", "audioOnly"].includes(option.group)) return null;
  return { file, duration, video: video && option.group === "video" };
}

/** The only file is audio or video with tracks to save as files of their own. */
export function canExtract(state: QueueState): boolean {
  const ready = readyItems(state);
  if (ready.length !== 1 || state.items.length !== 1) return false;
  const media = ready[0].file!.media;
  return MEDIA_KINDS.includes(media.kind) && media.tracks.length > 1;
}

/** The only file is a video made into a video (not an animation) or an edited copy: audio can be added to it. */
export function audioFileScope(state: QueueState): boolean {
  const scope = trimScope(state);
  if (!scope?.video || state.extract) return false;
  const format = state.editCopy ? scope.file.edit : state.target;
  if (format == null || ANIMATIONS.includes(format)) return false;
  return state.editCopy || commonTargets(state.items).find((o) => o.format === format)?.group === "video";
}

/** The only file has audio (its own or added) made into audio or video, or an edited copy: its loudness can change. */
export function volumeScope(state: QueueState): boolean {
  const scope = trimScope(state);
  if (!scope || state.extract) return false;
  const added = audioFileScope(state) && state.audioFile != null;
  if (scope.file.media.audio.length === 0 && !added) return false;
  const format = state.editCopy ? scope.file.edit : state.target;
  return format != null && !ANIMATIONS.includes(format);
}

/** Several audio or video files, all ready, made into audio or video (not an animation): they can be joined. */
export function canJoin(state: QueueState): boolean {
  const ready = readyItems(state);
  if (state.compress || state.editCopy || state.extract || ready.length < 2 || ready.length !== state.items.length) return false;
  if (!ready.every((i) => MEDIA_KINDS.includes(i.file!.media.kind))) return false;
  const option = commonTargets(state.items).find((o) => o.format === state.target);
  return option != null && ["video", "audio", "audioOnly"].includes(option.group) && !ANIMATIONS.includes(option.format);
}

/** The files are joined into one. */
export function joins(state: QueueState): boolean {
  return state.join && canJoin(state);
}

/** Formats of animations, which have no audio. */
const ANIMATIONS: TargetFormat[] = ["gif", "animatedWebp", "apng"];

/** The tracks kept, as shown: chosen, or the default ones. */
export function currentTracks(state: QueueState): TrackChoice[] {
  const scope = trackScope(state);
  if (!scope) return [];
  return state.tracks ?? defaultTracks(scope.file.media.tracks, state.editCopy);
}

/** Track choices sent to the core (`null`: nothing chosen). */
function mediaOf(state: QueueState, copy: boolean): MediaEdit | null {
  const tracks = trackScope(state) ? state.tracks : null;
  const kept = trimScope(state) && isTrimmed(state.trim) ? state.trim! : null;
  // Fast for a copy, precise for a conversion (which encodes anyway), unless chosen.
  const trim = kept && { startSecs: kept.start, endSecs: kept.end, precise: kept.precise ?? !copy };
  const audioFile = audioFileScope(state) ? state.audioFile : null;
  const volume = volumeScope(state) ? state.volume : null;
  if (!copy && tracks == null && trim == null && audioFile == null && volume == null) return null;
  return { tracks, trim, audioFile, volume, copy, label: copy ? EDIT_LABEL : null };
}

/** An edited copy changes something in the pictures or in the tracks. */
function copyChanged(state: QueueState): boolean {
  const scope = editScope(state);
  if (scope && editChanged(state.edit, scope)) return true;
  if (trimScope(state) && isTrimmed(state.trim)) return true;
  if (audioFileScope(state) && state.audioFile != null) return true;
  if (volumeScope(state) && state.volume != null) return true;
  const tracks = trackScope(state);
  return tracks != null && state.tracks != null && !sameTracks(state.tracks, defaultTracks(tracks.file.media.tracks, true));
}

/** The only file is a PDF: as a PDF target, its pages can be edited and split. */
export function lonePdf(items: QueueItem[]): QueueItem | null {
  const files = items.filter((i) => i.file && i.status !== "invalid");
  return files.length === 1 && files[0].file!.media.kind === "document" ? files[0] : null;
}

/**
 * Files combined into one PDF whose pages can be arranged one by one: PDFs
 * and photos (a Word document has pages only once it is laid out).
 */
export function arrangesPages(state: QueueState): boolean {
  return combines(state) && readyItems(state).every((i) => ["document", "image"].includes(i.file!.media.kind));
}

/** The PDF being made can be edited page by page: a lone PDF, or files combined. */
export function editsPdf(state: QueueState): boolean {
  return state.target === "pdf" && (lonePdf(state.items) != null || arrangesPages(state));
}

/** Files whose pages are in the page editor, in list order (a photo is one page). */
export function pageSources(state: QueueState): PageSource[] {
  return readyItems(state).map((i) => ({
    key: i.key,
    count: i.file!.media.kind === "document" ? (i.file!.media.document?.pages ?? 1) : 1,
  }));
}

/** The edited pages for the files now in the list (`null`: untouched). */
function currentPages(state: QueueState): EditorPage[] | null {
  return state.pages && reconcile(state.pages, pageSources(state));
}

export function requestOf(state: QueueState): ConversionRequest | null {
  if (!state.target) return null;
  const edit = editsPdf(state);
  const sources = pageSources(state);
  const pages = currentPages(state);
  return {
    target: state.target,
    preset: state.preset,
    advanced: state.advanced,
    frames: state.frames,
    removeLocation: state.removeLocation,
    // Page edits and splits: a lone PDF, or files combined page by page.
    // A password protects new PDFs only.
    pdf: {
      ...state.pdf,
      ...(edit
        ? { layout: layoutOf(pages, sources), splitAfter: cutPositions(pages, state.cuts, sources) }
        : { layout: null, split: "none", splitAfter: [] }),
      protection: state.target === "pdf" ? state.pdf.protection : null,
      watermark: state.target === "pdf" ? state.pdf.watermark : null,
      pageNumbers: state.target === "pdf" ? state.pdf.pageNumbers : null,
      ocr: state.target === "pdf" && state.pdf.ocr,
    },
    edit: editOf(state),
    media: mediaOf(state, false),
    join: joins(state) ? { files: readyItems(state).map((i) => i.file!.media), label: null } : null,
  };
}

function editOf(state: QueueState) {
  const scope = editScope(state);
  return scope ? pictureEditOf(state.edit, scope, state.editCopy ? EDIT_LABEL : null) : null;
}

/**
 * The request for one file: the shared choices (for files combined page by
 * page, with the pages of that file only: its warnings, its share of the
 * size; `null` when none of its pages is kept), or a smaller copy in the
 * format of the file.
 */
export function requestFor(state: QueueState, item: QueueItem): ConversionRequest | null {
  if (state.extract) {
    if (!canExtract(state) || state.extractTracks.length === 0) return null;
    // Tracks are copied when they can be: a trim is fast unless chosen.
    const trim = mediaOf(state, true)?.trim ?? null;
    return {
      // The file's own format (only shown; each track has its own).
      target: item.file?.edit ?? item.file?.compress ?? "mkv",
      preset: state.preset,
      advanced: {},
      frames: null,
      removeLocation: state.removeLocation,
      pdf: null,
      media: trim ? { tracks: null, trim, audioFile: null, volume: null, copy: false, label: null } : null,
      extract: { tracks: state.extractTracks },
    };
  }
  if (state.editCopy) {
    const target = item.file?.edit;
    if (!target) return null;
    const picture = item.file!.media.kind === "image";
    return {
      target,
      preset: state.preset,
      advanced: state.advanced,
      frames: null,
      removeLocation: state.removeLocation,
      pdf: null,
      edit: picture ? editOf(state) : null,
      media: picture ? null : mediaOf(state, true),
    };
  }
  if (!state.compress) {
    const request = requestOf(state);
    // A join is planned once, as a whole, with the first file.
    if (joins(state)) return readyItems(state)[0]?.key === item.key ? request : null;
    if (!request?.pdf || !arrangesPages(state)) return request;
    const index = pageSources(state).findIndex((s) => s.key === item.key);
    const own = request.pdf.layout?.filter((p) => p.file === index).map((p) => ({ ...p, file: 0 })) ?? null;
    if (own && own.length === 0) return null;
    const photo = item.file?.media.kind === "image";
    return { ...request, pdf: { ...request.pdf, layout: photo ? null : own, split: "none", splitAfter: [] } };
  }
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

/** Items that take part in the conversion: ready, and not left out of the PDF being made. */
export function plannedItems(state: QueueState): QueueItem[] {
  return readyItems(state).filter((i) => requestFor(state, i) != null);
}

/** Documents an edited PDF becomes, by the same rules as the core. */
export function editedDocuments(state: QueueState): number {
  const sources = pageSources(state);
  const pages = currentPages(state);
  const kept = pages ? keptCount(pages) : sources.reduce((n, s) => n + s.count, 0);
  if (state.pdf.split === "every") return Math.max(1, Math.ceil(kept / Math.max(1, state.pdf.splitEvery)));
  if (state.pdf.split === "after") return cutPositions(pages, state.cuts, sources).length + 1;
  return 1;
}

/**
 * The edited PDF differs from the original: pages changed, several
 * documents, something drawn over the pages, text read by OCR or a password.
 */
export function pdfChanged(state: QueueState): boolean {
  const { watermark, pageNumbers, protection, ocr } = state.pdf;
  const added =
    (watermark != null && (watermark.picture != null || watermark.text.trim() !== "")) ||
    pageNumbers != null ||
    protection != null ||
    ocr;
  return added || isEdited(currentPages(state), pageSources(state)) || editedDocuments(state) > 1;
}

/**
 * Every file goes into one PDF: always with PDFs among them, with pictures
 * unless turned off; Word documents alone become one PDF each.
 */
export function combines(state: QueueState): boolean {
  const ready = readyItems(state);
  const kinds = new Set(ready.map((i) => i.file!.media.kind));
  return (
    joins(state) ||
    (state.target === "pdf" && ready.length > 1 && (kinds.has("document") || (state.pdf.combine && kinds.has("image"))))
  );
}

/** Items shown as results: a combined PDF is one result, shared by every file. */
export function resultItems(state: QueueState): QueueItem[] {
  const finished = state.items.filter((i) => FINISHED.includes(i.status));
  return state.combined ? finished.slice(0, 1) : finished;
}

export function canStart(state: QueueState): boolean {
  const ready = readyItems(state);
  const planned = plannedItems(state);
  return (
    state.phase === "setup" &&
    (state.target != null ||
      (state.compress && canCompress(ready)) ||
      (state.editCopy && canEditCopy(ready)) ||
      (state.extract && canExtract(state))) &&
    // An edited copy with nothing edited would be pointless.
    (!state.editCopy || copyChanged(state)) &&
    planned.length > 0 &&
    !state.items.some((i) => i.status === "analyzing") &&
    planned.every((i) => i.plan && !i.planError) &&
    // A copy of a lone PDF with nothing changed would be pointless (unless
    // it was opened with a password: the copy has none).
    (!editsPdf(state) || combines(state) || pdfChanged(state) || lonePdf(state.items)?.file?.media.document?.locked === true) &&
    // A password is typed twice, the same.
    (state.target !== "pdf" || state.pdf.protection?.password !== "")
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
  if (state.editCopy && !canEditCopy(state.items)) {
    return invalidate({ ...state, editCopy: false });
  }
  if (state.extract && !canExtract(state)) {
    return invalidate({ ...state, extract: false, extractTracks: [] });
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
      return {
        ...base,
        phase: "setup",
        notice: null,
        nextKey,
        items: [...base.items, ...added],
        edit: { ...base.edit, crop: null },
        tracks: null,
        trim: null,
        audioFile: null,
        volume: null,
        // Tracks are extracted from one file only.
        extract: false,
        extractTracks: [],
      };
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

    case "unlocked": {
      const item = state.items.find((i) => i.key === action.key);
      if (!item || item.status !== "invalid" || state.phase !== "setup") return state;
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
      const next = withoutItems({ ...state, edit: { ...state.edit, crop: null }, tracks: null, trim: null, audioFile: null, volume: null, extractTracks: [] }, state.items.filter((i) => i.key !== action.key));
      // The pages of the file go with it; edits are forgotten once no PDF is made of the files left.
      if (!next.pages) return next;
      return invalidate(editsPdf(next) ? next : { ...next, pages: null, cuts: [] });
    }

    case "move": {
      // Order of the files in a combined PDF: edited pages move with their file.
      if (state.phase !== "setup") return state;
      const from = state.items.findIndex((i) => i.key === action.key);
      const to = from + action.delta;
      if (from < 0 || to < 0 || to >= state.items.length) return state;
      const items = [...state.items];
      [items[from], items[to]] = [items[to], items[from]];
      const next = { ...state, items };
      const order = pageSources(state).map((s) => s.key);
      const moved = pageSources(next).map((s) => s.key);
      if (!state.pages || order.indexOf(action.key) === moved.indexOf(action.key)) return next;
      return invalidate({ ...next, pages: moveFile(currentPages(state)!, order, action.key, action.delta) });
    }

    case "selectTarget":
      if (state.phase !== "setup" || (state.target === action.target && !state.compress && !state.editCopy && !state.extract)) return state;
      // Codec choices of another format may not apply: start from automatic.
      return invalidate({ ...state, target: action.target, compress: false, editCopy: false, extract: false, advanced: {}, notice: null });

    case "selectCompress":
      if (state.phase !== "setup" || state.compress || !canCompress(state.items)) return state;
      return invalidate({ ...state, compress: true, editCopy: false, extract: false, target: null, advanced: {}, notice: null });

    case "selectEditCopy":
      if (state.phase !== "setup" || state.editCopy || !canEditCopy(state.items)) return state;
      return invalidate({ ...state, editCopy: true, compress: false, extract: false, target: null, advanced: {}, notice: null });

    case "setAudioFile":
      if (state.phase !== "setup") return state;
      return invalidate({ ...state, audioFile: action.audioFile, notice: null });

    case "setJoin":
      if (state.phase !== "setup") return state;
      return invalidate({ ...state, join: action.join, notice: null });

    case "setVolume":
      if (state.phase !== "setup") return state;
      return invalidate({ ...state, volume: action.volume, notice: null });

    case "selectExtract":
      if (state.phase !== "setup" || state.extract || !canExtract(state)) return state;
      return invalidate({ ...state, extract: true, editCopy: false, compress: false, target: null, advanced: {}, notice: null });

    case "setExtract":
      if (state.phase !== "setup") return state;
      return invalidate({ ...state, extractTracks: action.tracks, notice: null });

    case "setEdit":
      if (state.phase !== "setup") return state;
      return invalidate({ ...state, edit: action.edit, notice: null });

    case "setTracks":
      if (state.phase !== "setup") return state;
      return invalidate({ ...state, tracks: action.tracks, notice: null });

    case "setTrim":
      if (state.phase !== "setup") return state;
      return invalidate({ ...state, trim: action.trim, notice: null });

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
