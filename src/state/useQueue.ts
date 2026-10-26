// Connects the queue state machine to the backend: analysis (a few files at a
// time), planning, sequential start, job events, cancellation, free-space check.
import { useCallback, useEffect, useReducer, useRef, useState } from "react";

import type { AdvancedOptions } from "../bindings/AdvancedOptions";
import type { FrameOptions } from "../bindings/FrameOptions";
import type { JobEvent } from "../bindings/JobEvent";
import type { QualityPreset } from "../bindings/QualityPreset";
import type { TargetFormat } from "../bindings/TargetFormat";
import { t } from "../i18n";
import { backend, toErrorPayload } from "../lib/api";
import { usePreferences } from "./preferences";
import {
  canStart,
  initialQueue,
  queueReducer,
  readyItems,
  requestOf,
  totalEstimate,
  type QueueItem,
} from "./queue";

const ANALYSIS_CONCURRENCY = 3;

const dirname = (path: string) => path.replace(/[\\/][^\\/]*$/, "");

export function useQueue() {
  const { prefs } = usePreferences();
  const [state, dispatch] = useReducer(queueReducer, prefs.defaultPreset, initialQueue);
  const [dragging, setDragging] = useState(false);
  const [lowSpace, setLowSpace] = useState<number | null>(null);
  const [closeRequested, setCloseRequested] = useState(false);

  const stateRef = useRef(state);
  stateRef.current = state;

  useEffect(() => dispatch({ type: "defaultPreset", preset: prefs.defaultPreset }), [prefs.defaultPreset]);

  // --- Analysis: a few FFprobe runs at a time ------------------------------------
  const analysisRequested = useRef(new Set<number>());
  const analysisWaiting = useRef<QueueItem[]>([]);
  const analysisRunning = useRef(0);
  const pump = useCallback(() => {
    while (analysisRunning.current < ANALYSIS_CONCURRENCY && analysisWaiting.current.length > 0) {
      const item = analysisWaiting.current.shift()!;
      analysisRunning.current++;
      backend
        .analyzeFile(item.path)
        .then(
          (file) => dispatch({ type: "analyzed", key: item.key, file }),
          (error) => dispatch({ type: "analyzeFailed", key: item.key, error: toErrorPayload(error) }),
        )
        .finally(() => {
          analysisRunning.current--;
          pump();
        });
    }
  }, []);
  useEffect(() => {
    for (const item of state.items) {
      if (item.status === "analyzing" && !analysisRequested.current.has(item.key)) {
        analysisRequested.current.add(item.key);
        analysisWaiting.current.push(item);
      }
    }
    pump();
  }, [state.items, pump]);

  // --- Planning: one plan per file for the current request -----------------------
  const planRequested = useRef(new Set<string>());
  useEffect(() => {
    const request = requestOf(state);
    if (state.phase !== "setup" || !request) return;
    const version = state.planVersion;
    for (const item of readyItems(state)) {
      const id = `${version}:${item.key}`;
      if (item.plan || item.planError || planRequested.current.has(id)) continue;
      planRequested.current.add(id);
      backend.planConversion(item.path, item.file!.media, request).then(
        (plan) => dispatch({ type: "planned", key: item.key, version, plan }),
        (error) => dispatch({ type: "planFailed", key: item.key, version, error: toErrorPayload(error) }),
      );
    }
  }, [state]);

  // --- Free space at the destination(s) -------------------------------------------
  useEffect(() => {
    setLowSpace(null);
    if (state.phase !== "setup") return;
    const items = readyItems(state);
    if (items.length === 0 || !totalEstimate(items)) return;
    const perDir = new Map<string, number>();
    for (const item of items) {
      const dir = prefs.outputDir ?? dirname(item.path);
      perDir.set(dir, (perDir.get(dir) ?? 0) + (item.plan?.estimate?.bytes ?? 0));
    }
    let stale = false;
    void Promise.all(
      [...perDir].map(async ([dir, needed]) => {
        const free = await backend.availableSpace(dir).catch(() => null);
        return free != null && free < needed * 1.05 ? free : null;
      }),
    ).then((results) => {
      const low = results.find((r) => r != null);
      if (!stale && low != null) setLowSpace(low);
    });
    return () => {
      stale = true;
    };
  }, [state, prefs.outputDir]);

  // --- Job events --------------------------------------------------------------------
  // Events may arrive before `start_conversion` returns the id: buffer them.
  const knownJobs = useRef(new Set<number>());
  const pendingEvents = useRef(new Map<number, JobEvent[]>());
  const cancelling = useRef(false);

  useEffect(() => {
    const unlisten = [
      backend.onJobUpdate((event) => {
        if (knownJobs.current.has(event.jobId)) dispatch({ type: "job", event });
        else pendingEvents.current.set(event.jobId, [...(pendingEvents.current.get(event.jobId) ?? []), event]);
      }),
      backend.onFileDrop({
        hover: setDragging,
        drop: (paths) => dispatch({ type: "add", paths }),
      }),
      backend.onCloseRequested(() => setCloseRequested(true)),
      backend.onOpenFiles((paths) => dispatch({ type: "add", paths })),
    ];
    // Files passed by "Open with Konvert" when the app was launched.
    backend.takeLaunchFiles().then(
      (paths) => paths.length > 0 && dispatch({ type: "add", paths }),
      () => {},
    );
    return () => {
      unlisten.forEach((p) => p.then((stop) => stop()));
    };
  }, []);

  const start = useCallback(async () => {
    const s = stateRef.current;
    const request = requestOf(s);
    if (!canStart(s) || !request) return;
    const items = readyItems(s);
    cancelling.current = false;
    dispatch({ type: "start" });
    // The backend runs one job at a time; queue them all now, in order.
    for (const item of items) {
      if (cancelling.current) break;
      try {
        const jobId = await backend.startConversion(item.path, request);
        knownJobs.current.add(jobId);
        dispatch({ type: "started", key: item.key, jobId });
        const early = pendingEvents.current.get(jobId) ?? [];
        pendingEvents.current.delete(jobId);
        early.forEach((event) => dispatch({ type: "job", event }));
        if (cancelling.current) void backend.cancelConversion(jobId);
      } catch (error) {
        dispatch({ type: "startFailed", key: item.key, error: toErrorPayload(error) });
      }
    }
  }, []);

  const cancelAll = useCallback(() => {
    const s = stateRef.current;
    if (s.phase !== "converting") return;
    cancelling.current = true;
    dispatch({ type: "cancelAll" });
    for (const item of s.items) {
      if (item.jobId != null && (item.status === "queued" || item.status === "running")) {
        void backend.cancelConversion(item.jobId);
      }
    }
  }, []);

  const chooseFiles = useCallback(async () => {
    const paths = await backend.pickFiles(t("drop.pickerTitle"), t("drop.pickerFilter"));
    if (paths && paths.length > 0) dispatch({ type: "add", paths });
  }, []);

  return {
    state,
    dragging,
    lowSpace,
    closeRequested,
    dismissClose: () => setCloseRequested(false),
    confirmClose: () => void backend.confirmClose(),
    addFiles: (paths: string[]) => dispatch({ type: "add", paths }),
    chooseFiles,
    remove: (key: number) => dispatch({ type: "remove", key }),
    selectTarget: (target: TargetFormat) => dispatch({ type: "selectTarget", target }),
    selectPreset: (preset: QualityPreset) => dispatch({ type: "selectPreset", preset }),
    setAdvanced: (advanced: AdvancedOptions) => dispatch({ type: "setAdvanced", advanced }),
    setFrames: (frames: FrameOptions) => dispatch({ type: "setFrames", frames }),
    setRemoveLocation: (removeLocation: boolean) => dispatch({ type: "setRemoveLocation", removeLocation }),
    start,
    cancelAll,
    back: () => dispatch({ type: "back" }),
    reset: () => dispatch({ type: "reset" }),
    openOutput: (jobId: number) => void backend.openOutput(jobId).catch(() => {}),
    revealOutput: (jobId: number) => void backend.revealOutput(jobId).catch(() => {}),
  };
}

export type QueueController = ReturnType<typeof useQueue>;
