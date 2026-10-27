import { describe, expect, it } from "vitest";
import {
  allWarnings,
  canCompress,
  canStart,
  combines,
  commonTargets,
  counts,
  DEFAULT_FRAMES,
  DEFAULT_PDF,
  editedDocuments,
  editsPdf,
  initialQueue,
  overallProgress,
  queueReducer as reduce,
  requestFor,
  requestOf,
  resultItems,
  totalEstimate,
  type QueueAction,
  type QueueState,
} from "./queue";
import type { ConversionPlan } from "../bindings/ConversionPlan";
import type { FileAnalysis } from "../bindings/FileAnalysis";
import type { TargetFormat } from "../bindings/TargetFormat";
import type { TargetOption } from "../bindings/TargetOption";
import { initialPages, movePage, rotatePage, toggleRemoved } from "./pages";

const option = (format: TargetFormat, group: TargetOption["group"]): TargetOption => ({
  format,
  label: format.toUpperCase(),
  extension: format,
  group,
  lossless: false,
  secondary: false,
  sameAsInput: false,
  kind:
    group === "video"
      ? "video"
      : group === "document"
        ? "document"
        : ["image", "frames", "pages"].includes(group)
          ? "image"
          : "audio",
  videoCodecs: [],
  audioCodecs: [],
});

function file(name: string, kind: "audio" | "videoWithAudio" | "image" | "document", duration: number | null): FileAnalysis {
  const targets =
    kind === "audio"
      ? [option("mp3", "audio"), option("flac", "audio")]
      : kind === "image"
        ? [option("jpg", "image"), option("png", "image"), option("webp", "image"), option("pdf", "document")]
        : kind === "document"
          ? [option("png", "pages"), option("jpg", "pages"), option("pdf", "document")]
          : [
            option("mp4", "video"),
            option("mkv", "video"),
            option("mp3", "audioOnly"),
            option("flac", "audioOnly"),
            option("jpg", "frames"),
            option("png", "frames"),
          ];
  return {
    path: `C:\\m\\${name}`,
    name,
    media: {
      container: "x",
      kind,
      durationSecs: duration,
      sizeBytes: 100,
      bitRate: null,
      audio: [],
      video: [],
      coverArt: null,
      subtitleStreams: 0,
      hasTags: false,
      picture: kind === "image" ? { alpha: false, alphaStream: null, gridGroup: null, exif: false, exifCopyable: false } : null,
      hasLocation: false,
      document: kind === "document" ? { pages: 4, widthPt: 595, heightPt: 842, extras: false } : null,
    },
    targets,
    // Like the core: every file keeps its own format, except HEIC photos.
    compress: kind === "document" ? "pdf" : name.endsWith(".heic") ? null : (name.split(".").pop() as TargetFormat),
  };
}

const plan = (target: TargetFormat, bytes: number, approximate = false, warnings: ConversionPlan["warnings"] = []): ConversionPlan => ({
  target,
  videoStream: null,
  video: null,
  audioStream: 0,
  audio: null,
  coverArtStream: null,
  image: null,
  frames: null,
  removeLocation: false,
  document: null,
  warnings,
  estimate: { bytes, approximate },
});

const run = (state: QueueState, ...actions: QueueAction[]) => actions.reduce(reduce, state);

/** Two analysed files (audio + video), target MP3, both planned. */
function readyBatch(): QueueState {
  let s = run(initialQueue(), { type: "add", paths: ["C:\\m\\a.wav", "C:\\m\\b.mp4"] });
  s = run(
    s,
    { type: "analyzed", key: 1, file: file("a.wav", "audio", 100) },
    { type: "analyzed", key: 2, file: file("b.mp4", "videoWithAudio", 300) },
    { type: "selectTarget", target: "mp3" },
  );
  return run(
    s,
    { type: "planned", key: 1, version: s.planVersion, plan: plan("mp3", 1000, false, ["losslessToLossy"]) },
    { type: "planned", key: 2, version: s.planVersion, plan: plan("mp3", 3000, true, ["lossyReencode", "losslessToLossy"]) },
  );
}

function converting(): QueueState {
  return run(readyBatch(), { type: "start" }, { type: "started", key: 1, jobId: 11 }, { type: "started", key: 2, jobId: 12 });
}

describe("queue state machine", () => {
  it("adds files once and analyses them", () => {
    let s = run(initialQueue(), { type: "add", paths: ["C:\\m\\a.wav", "C:\\m\\a.wav"] });
    s = reduce(s, { type: "add", paths: ["C:\\m\\a.wav"] });
    expect(s.items).toHaveLength(1);
    expect(s).toMatchObject({ phase: "setup", items: [{ status: "analyzing", name: "a.wav" }] });
    s = reduce(s, { type: "analyzed", key: 1, file: file("a.wav", "audio", 10) });
    expect(s.items[0].status).toBe("ready");
  });

  it("offers only formats common to every file, as audio formats for mixed batches", () => {
    const s = readyBatch();
    const targets = commonTargets(s.items);
    expect(targets.map((t) => t.format)).toEqual(["mp3", "flac"]);
    expect(targets.every((t) => t.group === "audio")).toBe(true);
  });

  it("converts photos together, never together with audio or video", () => {
    let s = run(initialQueue(), { type: "add", paths: ["a.jpg", "b.heic"] });
    s = run(
      s,
      { type: "analyzed", key: 1, file: file("a.jpg", "image", null) },
      { type: "analyzed", key: 2, file: file("b.heic", "image", null) },
    );
    const targets = commonTargets(s.items);
    expect(targets.map((t) => t.format)).toEqual(["jpg", "png", "webp", "pdf"]);
    expect(targets.filter((t) => t.group === "image")).toHaveLength(3);

    s = run(s, { type: "add", paths: ["c.wav"] }, { type: "analyzed", key: 3, file: file("c.wav", "audio", 10) });
    expect(commonTargets(s.items)).toEqual([]);
  });

  it("never mixes converted photos with frames of a video", () => {
    let s = run(initialQueue(), { type: "add", paths: ["a.jpg", "b.mp4", "c.mp4"] });
    s = run(
      s,
      { type: "analyzed", key: 2, file: file("b.mp4", "videoWithAudio", 10) },
      { type: "analyzed", key: 3, file: file("c.mp4", "videoWithAudio", 20) },
    );
    // Two videos: frames are common.
    expect(commonTargets(s.items).filter((t) => t.group === "frames").map((t) => t.format)).toEqual(["jpg", "png"]);
    s = reduce(s, { type: "analyzed", key: 1, file: file("a.jpg", "image", null) });
    expect(commonTargets(s.items)).toEqual([]);
  });

  it("asks to remove the location only when told, and plans again", () => {
    let s = run(initialQueue(), { type: "add", paths: ["a.jpg"] });
    s = run(s, { type: "analyzed", key: 1, file: file("a.jpg", "image", null) }, { type: "selectTarget", target: "webp" });
    expect(requestOf(s)?.removeLocation).toBe(false);
    const version = s.planVersion;
    s = reduce(s, { type: "setRemoveLocation", removeLocation: true });
    expect(s.planVersion).toBe(version + 1);
    expect(requestOf(s)?.removeLocation).toBe(true);
    // A new set of files starts again from the default.
    s = reduce(s, { type: "reset" });
    expect(s.removeLocation).toBe(false);
  });

  it("sends the frame options with the request and plans again when they change", () => {
    let s = run(initialQueue(), { type: "add", paths: ["b.mp4"] });
    s = run(s, { type: "analyzed", key: 1, file: file("b.mp4", "videoWithAudio", 10) }, { type: "selectTarget", target: "png" });
    expect(requestOf(s)?.frames).toEqual({ mode: "single", atSecs: null, everySecs: null });
    s = reduce(s, { type: "planned", key: 1, version: s.planVersion, plan: plan("png", 10, true) });
    const version = s.planVersion;
    s = reduce(s, { type: "setFrames", frames: { mode: "sequence", atSecs: null, everySecs: 2 } });
    expect(s.planVersion).toBe(version + 1);
    expect(s.items[0].plan).toBeNull();
    expect(requestOf(s)?.frames).toEqual({ mode: "sequence", atSecs: null, everySecs: 2 });
    // Another picture format keeps the choice.
    s = reduce(s, { type: "selectTarget", target: "jpg" });
    expect(s.frames.mode).toBe("sequence");
  });

  it("weights photos without a duration equally in the overall progress", () => {
    let s = run(initialQueue(), { type: "add", paths: ["a.jpg", "b.jpg"] });
    s = run(
      s,
      { type: "analyzed", key: 1, file: file("a.jpg", "image", null) },
      { type: "analyzed", key: 2, file: file("b.jpg", "image", null) },
      { type: "selectTarget", target: "webp" },
    );
    s = run(
      s,
      { type: "planned", key: 1, version: s.planVersion, plan: plan("webp", 10, true) },
      { type: "planned", key: 2, version: s.planVersion, plan: plan("webp", 10, true) },
      { type: "start" },
      { type: "started", key: 1, jobId: 1 },
      { type: "started", key: 2, jobId: 2 },
      { type: "job", event: { jobId: 1, update: { state: "done", outcome: { output: "a.webp", sizeBytes: 1, files: 1, elapsed: 1 } } } },
    );
    expect(overallProgress(s)).toBeCloseTo(0.5);
  });

  it("clears a target that is no longer common when a file is added", () => {
    let s = run(initialQueue(), { type: "add", paths: ["b.mp4"] });
    s = run(s, { type: "analyzed", key: 1, file: file("b.mp4", "videoWithAudio", 10) }, { type: "selectTarget", target: "mp4" });
    s = run(s, { type: "add", paths: ["a.wav"] }, { type: "analyzed", key: 2, file: file("a.wav", "audio", 10) });
    expect(s.target).toBe(null);
  });

  it("marks files without targets or failing analysis as invalid and skips them", () => {
    let s = run(initialQueue(), { type: "add", paths: ["x.txt", "a.wav"] });
    s = run(
      s,
      { type: "analyzeFailed", key: 1, error: { code: "not_media", details: null } },
      { type: "analyzed", key: 2, file: file("a.wav", "audio", 10) },
      { type: "selectTarget", target: "mp3" },
    );
    s = reduce(s, { type: "planned", key: 2, version: s.planVersion, plan: plan("mp3", 1) });
    expect(canStart(s)).toBe(true);
    s = reduce(s, { type: "start" });
    expect(s.items.map((i) => i.status)).toEqual(["invalid", "queued"]);
  });

  it("discards plans computed for an older request", () => {
    let s = readyBatch();
    const old = s.planVersion;
    s = reduce(s, { type: "selectPreset", preset: "small" });
    s = reduce(s, { type: "planned", key: 1, version: old, plan: plan("mp3", 1) });
    expect(s.items[0].plan).toBe(null);
    expect(canStart(s)).toBe(false);
    expect(requestOf(s)).toEqual({
      target: "mp3",
      preset: "small",
      advanced: {},
      frames: DEFAULT_FRAMES,
      removeLocation: false,
      pdf: DEFAULT_PDF,
    });
  });

  it("changing format resets the advanced options", () => {
    let s = reduce(readyBatch(), { type: "setAdvanced", advanced: { audioBitrateKbps: 320 } });
    expect(s.advanced).toEqual({ audioBitrateKbps: 320 });
    s = reduce(s, { type: "selectTarget", target: "flac" });
    expect(s.advanced).toEqual({});
  });

  it("aggregates estimates and warnings", () => {
    const s = readyBatch();
    expect(totalEstimate(s.items)).toEqual({ bytes: 4000, approximate: true });
    expect(allWarnings(s.items)).toEqual(["losslessToLossy", "lossyReencode"]);
  });

  it("tracks per-file and overall progress, weighted by duration", () => {
    let s = converting();
    const progress = { fraction: 0.5, etaSecs: 1, speed: 2, processedSecs: 50, outputBytes: 1 };
    s = reduce(s, { type: "job", event: { jobId: 11, update: { state: "progress", progress } } });
    // 100 s × 0.5 of 400 s.
    expect(overallProgress(s)).toBeCloseTo(0.125);
    const outcome = { output: "C:\\m\\a.mp3", sizeBytes: 1, files: 1, elapsed: 1 };
    s = reduce(s, { type: "job", event: { jobId: 11, update: { state: "done", outcome } } });
    expect(overallProgress(s)).toBeCloseTo(0.25);
    expect(s.phase).toBe("converting");
    s = reduce(s, { type: "job", event: { jobId: 12, update: { state: "failed", error: { code: "disk_full", details: null } } } });
    expect(s.phase).toBe("finished");
    expect(counts(s)).toMatchObject({ done: 1, failed: 1, total: 2 });
  });

  it("returns to the choices when everything was cancelled", () => {
    let s = reduce(converting(), { type: "cancelAll" });
    expect(s.cancelling).toBe(true);
    s = run(
      s,
      { type: "job", event: { jobId: 11, update: { state: "cancelled" } } },
      { type: "job", event: { jobId: 12, update: { state: "cancelled" } } },
    );
    expect(s).toMatchObject({ phase: "setup", notice: "cancelled", cancelling: false });
    expect(s.items.every((i) => i.status === "ready")).toBe(true);
    expect(canStart(s)).toBe(true);
  });

  it("cancels files that were not handed to the backend yet", () => {
    let s = run(readyBatch(), { type: "start" }, { type: "started", key: 1, jobId: 11 });
    s = reduce(s, { type: "cancelAll" });
    expect(s.items.map((i) => i.status)).toEqual(["queued", "cancelled"]);
    expect(s.phase).toBe("converting");
    s = reduce(s, { type: "job", event: { jobId: 11, update: { state: "cancelled" } } });
    expect(s).toMatchObject({ phase: "setup", notice: "cancelled" });
  });

  it("ignores new files and resets while converting", () => {
    const s = converting();
    expect(reduce(s, { type: "add", paths: ["new.wav"] })).toBe(s);
    expect(reduce(s, { type: "reset" })).toBe(s);
  });

  it("starts over with the default preset after finishing", () => {
    let s = run(initialQueue("small"), { type: "add", paths: ["a.wav"] });
    s = run(s, { type: "analyzed", key: 1, file: file("a.wav", "audio", 1) }, { type: "selectTarget", target: "mp3" });
    s = run(s, { type: "planned", key: 1, version: s.planVersion, plan: plan("mp3", 1) }, { type: "start" }, { type: "started", key: 1, jobId: 5 });
    s = reduce(s, { type: "job", event: { jobId: 5, update: { state: "done", outcome: { output: "a.mp3", sizeBytes: 1, files: 1, elapsed: 1 } } } });
    expect(s.phase).toBe("finished");
    s = reduce(s, { type: "add", paths: ["b.wav"] });
    expect(s).toMatchObject({ phase: "setup", preset: "small", target: null });
    expect(s.items.map((i) => i.name)).toEqual(["b.wav"]);
  });

  it("offers a PDF for pictures and PDFs together, and for editing a lone PDF", () => {
    let s = run(initialQueue(), { type: "add", paths: ["a.pdf"] });
    s = reduce(s, { type: "analyzed", key: 1, file: file("a.pdf", "document", null) });
    expect(commonTargets(s.items).map((t) => [t.format, t.group])).toEqual([
      ["png", "pages"],
      ["jpg", "pages"],
      ["pdf", "document"],
    ]);
    s = reduce(s, { type: "selectTarget", target: "pdf" });
    expect(editsPdf(s)).toBe(true);
    s = run(s, { type: "add", paths: ["b.jpg"] }, { type: "analyzed", key: 2, file: file("b.jpg", "image", null) });
    expect(commonTargets(s.items).map((t) => [t.format, t.group])).toEqual([["pdf", "document"]]);
    expect(editsPdf(s)).toBe(false);
  });

  it("edits the pages of a lone PDF, and only of that PDF", () => {
    let s = run(initialQueue(), { type: "add", paths: ["a.pdf"] });
    s = run(s, { type: "analyzed", key: 1, file: file("a.pdf", "document", null) }, { type: "selectTarget", target: "pdf" });
    const planned = (state: QueueState, parts: number[] = []) =>
      reduce(state, {
        type: "planned",
        key: 1,
        version: state.planVersion,
        plan: { ...plan("pdf", 100), document: { pages: [1, 2, 3, 4], rotations: [], parts, dpi: 0, placement: null } },
      });
    // Nothing changed yet: nothing to save.
    s = planned(s);
    expect(canStart(s)).toBe(false);
    expect(requestOf(s)?.pdf).toMatchObject({ layout: null, split: "none", splitAfter: [] });

    // Page 4 moves to the front turned, page 2 goes; a cut after page 4.
    const pages = toggleRemoved(rotatePage(movePage(initialPages(4), 3, 0), 0, 90), 2);
    s = run(s, { type: "setPages", pages, cuts: [4] }, { type: "setPdf", pdf: { ...s.pdf, split: "after" } });
    expect(s.items[0].plan).toBeNull();
    expect(requestOf(s)?.pdf).toMatchObject({
      layout: [
        { number: 4, rotation: 90 },
        { number: 1, rotation: 0 },
        { number: 3, rotation: 0 },
      ],
      split: "after",
      splitAfter: [1],
    });
    expect(editedDocuments(s)).toBe(2);
    expect(editedDocuments({ ...s, pdf: { ...s.pdf, split: "every", splitEvery: 2 } })).toBe(2);
    expect(editedDocuments({ ...s, pdf: { ...s.pdf, split: "every", splitEvery: 1 } })).toBe(3);
    s = planned(s, [1, 2]);
    expect(canStart(s)).toBe(true);

    // With another file the edits are not sent; removing the PDF forgets them.
    s = run(s, { type: "add", paths: ["b.pdf"] }, { type: "analyzed", key: 2, file: file("b.pdf", "document", null) });
    expect(requestOf(s)?.pdf).toMatchObject({ layout: null, split: "none", splitAfter: [] });
    s = reduce(s, { type: "remove", key: 1 });
    expect(s).toMatchObject({ pages: null, cuts: [] });
  });

  it("combines every file into one job, in the order chosen", () => {
    let s = run(initialQueue(), { type: "add", paths: ["a.jpg", "b.jpg", "c.pdf"] });
    s = run(
      s,
      { type: "analyzed", key: 1, file: file("a.jpg", "image", null) },
      { type: "analyzed", key: 2, file: file("b.jpg", "image", null) },
      { type: "analyzed", key: 3, file: file("c.pdf", "document", null) },
      { type: "selectTarget", target: "pdf" },
    );
    // Pages follow the list: the PDF first, then the photos.
    s = run(s, { type: "move", key: 3, delta: -1 }, { type: "move", key: 3, delta: -1 }, { type: "move", key: 3, delta: -1 });
    expect(s.items.map((i) => i.name)).toEqual(["c.pdf", "a.jpg", "b.jpg"]);
    s = run(s, { type: "move", key: 2, delta: 1 });
    expect(s.items.map((i) => i.key)).toEqual([3, 1, 2]);
    // A PDF among the files: always one document, even with "one PDF" off.
    s = reduce(s, { type: "setPdf", pdf: { ...DEFAULT_PDF, combine: false } });
    expect(combines(s)).toBe(true);

    for (const key of [1, 2, 3]) s = reduce(s, { type: "planned", key, version: s.planVersion, plan: plan("pdf", 100) });
    s = reduce(s, { type: "start" });
    expect(s.combined).toBe(true);
    s = run(s, ...[3, 1, 2].map((key): QueueAction => ({ type: "started", key, jobId: 9 })));
    const progress = { fraction: 0.4, etaSecs: null, speed: null, processedSecs: null, outputBytes: null };
    s = reduce(s, { type: "job", event: { jobId: 9, update: { state: "progress", progress } } });
    expect(s.items.every((i) => i.status === "running" && i.progress?.fraction === 0.4)).toBe(true);
    expect(overallProgress(s)).toBeCloseTo(0.4);
    // Moving files is over once the conversion started.
    expect(reduce(s, { type: "move", key: 1, delta: -1 })).toBe(s);

    const outcome = { output: "C:\m\c.pdf", sizeBytes: 300, files: 1, elapsed: 1 };
    s = reduce(s, { type: "job", event: { jobId: 9, update: { state: "done", outcome } } });
    expect(s.phase).toBe("finished");
    expect(resultItems(s)).toHaveLength(1);
    expect(resultItems(s)[0].outcome).toEqual(outcome);
    s = reduce(s, { type: "back" });
    expect(s).toMatchObject({ phase: "setup", combined: false });
  });

  it("makes one PDF per photo when asked", () => {
    let s = run(initialQueue(), { type: "add", paths: ["a.jpg", "b.jpg"] });
    s = run(
      s,
      { type: "analyzed", key: 1, file: file("a.jpg", "image", null) },
      { type: "analyzed", key: 2, file: file("b.jpg", "image", null) },
      { type: "selectTarget", target: "pdf" },
    );
    expect(combines(s)).toBe(true);
    const version = s.planVersion;
    s = reduce(s, { type: "setPdf", pdf: { ...s.pdf, combine: false } });
    expect(s.planVersion).toBe(version + 1);
    expect(combines(s)).toBe(false);
    expect(requestOf(s)?.pdf?.combine).toBe(false);
    // Another format: nothing to combine.
    s = run(s, { type: "setPdf", pdf: DEFAULT_PDF }, { type: "selectTarget", target: "png" });
    expect(combines(s)).toBe(false);
  });

  it("compresses every file in its own format", () => {
    let s = run(initialQueue(), { type: "add", paths: ["a.pdf", "b.jpg", "c.mp4"] });
    s = run(
      s,
      { type: "analyzed", key: 1, file: file("a.pdf", "document", null) },
      { type: "analyzed", key: 2, file: file("b.jpg", "image", null) },
      { type: "analyzed", key: 3, file: file("c.mp4", "videoWithAudio", 60) },
    );
    expect(canCompress(s.items)).toBe(true);
    // No format is common to the three, yet each can be made smaller.
    expect(commonTargets(s.items)).toEqual([]);
    const version = s.planVersion;
    s = reduce(s, { type: "selectCompress" });
    expect(s).toMatchObject({ compress: true, target: null, planVersion: version + 1 });
    expect(requestOf(s)).toBeNull();
    expect(s.items.map((i) => requestFor(s, i)?.target)).toEqual(["pdf", "jpg", "mp4"]);
    expect(requestFor(s, s.items[0])?.compress).toEqual({ level: "strong", maxBytes: null, pagesAsPictures: false, label: null });
    expect(canStart(s)).toBe(false);
    s = run(
      s,
      ...s.items.map((item): QueueAction => ({ type: "planned", key: item.key, version: s.planVersion, plan: plan(item.file!.compress!, 50) })),
    );
    expect(canStart(s)).toBe(true);
    s = reduce(s, { type: "setCompression", compression: { ...s.compression, level: "max", maxBytes: 2_000_000 } });
    expect(canStart(s)).toBe(false);
    expect(requestFor(s, s.items[2])?.compress).toMatchObject({ level: "max", maxBytes: 2_000_000 });
    // Choosing a format leaves compression.
    s = reduce(s, { type: "remove", key: 3 });
    s = reduce(s, { type: "selectTarget", target: "pdf" });
    expect(s.compress).toBe(false);
    expect(requestFor(s, s.items[0])?.compress).toBeUndefined();
  });

  it("offers compression only when every file can be compressed", () => {
    let s = run(initialQueue(), { type: "add", paths: ["a.jpg", "b.heic"] });
    s = run(s, { type: "analyzed", key: 1, file: file("a.jpg", "image", null) }, { type: "selectCompress" });
    expect(s.compress).toBe(true);
    // A HEIC photo cannot be made smaller as HEIC: compression is turned off.
    s = reduce(s, { type: "analyzed", key: 2, file: file("b.heic", "image", null) });
    expect(canCompress(s.items)).toBe(false);
    expect(s.compress).toBe(false);
    expect(reduce(s, { type: "selectCompress" }).compress).toBe(false);
    s = reduce(s, { type: "remove", key: 2 });
    expect(reduce(s, { type: "selectCompress" }).compress).toBe(true);
  });
});
