import { describe, expect, it } from "vitest";
import {
  allWarnings,
  canStart,
  commonTargets,
  counts,
  DEFAULT_FRAMES,
  initialQueue,
  overallProgress,
  queueReducer as reduce,
  requestOf,
  totalEstimate,
  type QueueAction,
  type QueueState,
} from "./queue";
import type { ConversionPlan } from "../bindings/ConversionPlan";
import type { FileAnalysis } from "../bindings/FileAnalysis";
import type { TargetFormat } from "../bindings/TargetFormat";
import type { TargetOption } from "../bindings/TargetOption";

const option = (format: TargetFormat, group: TargetOption["group"]): TargetOption => ({
  format,
  label: format.toUpperCase(),
  extension: format,
  group,
  lossless: false,
  secondary: false,
  sameAsInput: false,
  kind: group === "video" ? "video" : group === "image" || group === "frames" ? "image" : "audio",
  videoCodecs: [],
  audioCodecs: [],
});

function file(name: string, kind: "audio" | "videoWithAudio" | "image", duration: number | null): FileAnalysis {
  const targets =
    kind === "audio"
      ? [option("mp3", "audio"), option("flac", "audio")]
      : kind === "image"
        ? [option("jpg", "image"), option("png", "image"), option("webp", "image")]
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
    },
    targets,
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
    expect(targets.map((t) => t.format)).toEqual(["jpg", "png", "webp"]);
    expect(targets.every((t) => t.group === "image")).toBe(true);

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
    expect(requestOf(s)).toEqual({ target: "mp3", preset: "small", advanced: {}, frames: DEFAULT_FRAMES, removeLocation: false });
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
});
