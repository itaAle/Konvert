import { describe, expect, it } from "vitest";
import { converterReducer as reduce, initialState, requestFor, type ConverterState } from "./converter";
import type { ConversionPlan } from "../bindings/ConversionPlan";
import type { FileAnalysis } from "../bindings/FileAnalysis";

const file: FileAnalysis = {
  path: "C:\\Musica\\brano.wav",
  name: "brano.wav",
  media: {
    container: "wav",
    kind: "audio",
    durationSecs: 10,
    sizeBytes: 1000,
    bitRate: null,
    audio: [],
    video: [],
    coverArt: null,
    subtitleStreams: 0,
  },
  targets: [],
};

const plan: ConversionPlan = {
  target: "mp3",
  videoStream: null,
  video: null,
  audioStream: 0,
  audio: null,
  coverArtStream: null,
  warnings: [],
  estimate: null,
};

function readyWithPlan(): ConverterState {
  let s = reduce(initialState, { type: "analyze", path: file.path });
  s = reduce(s, { type: "analyzed", file });
  s = reduce(s, { type: "selectTarget", target: "mp3" });
  return reduce(s, { type: "planned", target: "mp3", preset: "balanced", plan });
}

function converting(): ConverterState {
  let s = reduce(readyWithPlan(), { type: "start" });
  return reduce(s, { type: "started", jobId: 4 });
}

describe("converter state machine", () => {
  it("goes from analysis to a ready selection", () => {
    const s = readyWithPlan();
    expect(s.phase).toBe("ready");
    if (s.phase !== "ready") return;
    expect(s.sel.target).toBe("mp3");
    expect(s.sel.plan).toBe(plan);
    expect(requestFor(s.sel)).toEqual({ target: "mp3", preset: "balanced", advanced: {} });
  });

  it("ignores plans for a selection that changed", () => {
    let s = reduce(readyWithPlan(), { type: "selectPreset", preset: "small" });
    s = reduce(s, { type: "planned", target: "mp3", preset: "balanced", plan });
    expect(s.phase === "ready" && s.sel.plan).toBe(null);
  });

  it("cannot start without a target and a plan", () => {
    let s = reduce(initialState, { type: "analyze", path: file.path });
    s = reduce(s, { type: "analyzed", file });
    expect(reduce(s, { type: "start" })).toBe(s);
  });

  it("follows job events of its own job only", () => {
    let s = converting();
    s = reduce(s, { type: "job", event: { jobId: 4, update: { state: "running" } } });
    expect(s.phase === "converting" && s.status).toBe("running");
    const progress = { fraction: 0.5, etaSecs: 3, speed: 2, processedSecs: 5, outputBytes: 10 };
    const other = reduce(s, { type: "job", event: { jobId: 9, update: { state: "progress", progress } } });
    expect(other).toBe(s);
    s = reduce(s, { type: "job", event: { jobId: 4, update: { state: "progress", progress } } });
    expect(s.phase === "converting" && s.progress).toEqual(progress);
    const outcome = { output: "C:\\Musica\\brano.mp3", sizeBytes: 5, elapsed: 1 };
    s = reduce(s, { type: "job", event: { jobId: 4, update: { state: "done", outcome } } });
    expect(s.phase).toBe("done");
  });

  it("returns to the selection with a notice after cancelling", () => {
    let s = reduce(converting(), { type: "cancel" });
    expect(s.phase === "converting" && s.status).toBe("cancelling");
    // A late "running" does not hide the cancelling state.
    s = reduce(s, { type: "job", event: { jobId: 4, update: { state: "running" } } });
    expect(s.phase === "converting" && s.status).toBe("cancelling");
    s = reduce(s, { type: "job", event: { jobId: 4, update: { state: "cancelled" } } });
    expect(s).toMatchObject({ phase: "ready", notice: "cancelled" });
  });

  it("keeps the selection after an error so the user can retry", () => {
    let s = reduce(converting(), {
      type: "job",
      event: { jobId: 4, update: { state: "failed", error: { code: "disk_full", details: null } } },
    });
    expect(s).toMatchObject({ phase: "error", error: { code: "disk_full" } });
    s = reduce(s, { type: "back" });
    expect(s.phase === "ready" && s.sel.target).toBe("mp3");
  });

  it("does not drop a running conversion on reset or new files", () => {
    const s = converting();
    expect(reduce(s, { type: "reset" })).toBe(s);
    expect(reduce(s, { type: "analyze", path: "x.wav" })).toBe(s);
  });
});
