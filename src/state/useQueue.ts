// Connects the queue state machine to the backend: analysis (a few files at a
// time), planning, sequential start, job events, cancellation, free-space check.
import { useCallback, useEffect, useReducer, useRef, useState } from "react";

import type { AdvancedOptions } from "../bindings/AdvancedOptions";
import type { CompressOptions } from "../bindings/CompressOptions";
import type { ConversionRequest } from "../bindings/ConversionRequest";
import type { ErrorPayload } from "../bindings/ErrorPayload";
import type { FrameOptions } from "../bindings/FrameOptions";
import type { JobEvent } from "../bindings/JobEvent";
import type { PdfOptions } from "../bindings/PdfOptions";
import type { QualityPreset } from "../bindings/QualityPreset";
import type { TargetFormat } from "../bindings/TargetFormat";
import { t } from "../i18n";
import { backend, toErrorPayload } from "../lib/api";
import type { EditorPage } from "./pages";
import { usePreferences } from "./preferences";
import {
  canStart,
  combines,
  initialQueue,
  queueReducer,
  readyItems,
  requestFor,
  requestOf,
  totalEstimate,
  type QueueItem,
} from "./queue";
import type { EditState } from "./edit";

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
    if (state.phase !== "setup") return;
    const version = state.planVersion;
    for (const item of readyItems(state)) {
      const request = requestFor(state, item);
      const id = `${version}:${item.key}`;
      if (!request || item.plan || item.planError || planRequested.current.has(id)) continue;
      planRequested.current.add(id);
      // A plan does not need the password that will protect the PDF.
      const protection = request.pdf?.protection;
      const planned = protection ? { ...request, pdf: { ...request.pdf!, protection: { ...protection, password: "" } } } : request;
      backend.planConversion(item.path, item.file!.media, planned).then(
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
    // A combined PDF is saved next to the first file.
    const together = combines(state);
    for (const item of items) {
      const dir = prefs.outputDir ?? dirname(together ? items[0].path : item.path);
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
    if (!canStart(s)) return;
    // Pictures of PDF pages, split PDFs and smaller copies are named
    // "name-pagina-01", "name-compresso" in the interface language.
    const labelled = (request: ConversionRequest): ConversionRequest => ({
      ...request,
      pdf: request.pdf ? { ...request.pdf, pageLabel: t("pdf.pageLabel"), pagesLabel: t("pdf.pagesLabel") } : request.pdf,
      compress: request.compress ? { ...request.compress, label: t("compress.fileLabel") } : request.compress,
    });
    const items = readyItems(s);
    cancelling.current = false;
    dispatch({ type: "start" });
    // Every file goes into one PDF: a single job shared by all the items.
    const groups = combines(s) ? [items] : items.map((item) => [item]);
    // The backend runs one job at a time; queue them all now, in order.
    for (const group of groups) {
      if (cancelling.current) break;
      // A combined PDF gets the pages of every file (each file was planned with its own).
      const base = group.length > 1 ? requestOf(s) : requestFor(s, group[0]);
      if (!base) continue;
      const request = labelled(base);
      try {
        const jobId =
          group.length > 1
            ? await backend.startCombined(
                group.map((i) => i.path),
                request,
              )
            : await backend.startConversion(group[0].path, request);
        knownJobs.current.add(jobId);
        group.forEach((item) => dispatch({ type: "started", key: item.key, jobId }));
        const early = pendingEvents.current.get(jobId) ?? [];
        pendingEvents.current.delete(jobId);
        early.forEach((event) => dispatch({ type: "job", event }));
        if (cancelling.current) void backend.cancelConversion(jobId);
      } catch (error) {
        group.forEach((item) => dispatch({ type: "startFailed", key: item.key, error: toErrorPayload(error) }));
      }
    }
  }, []);

  const cancelAll = useCallback(() => {
    const s = stateRef.current;
    if (s.phase !== "converting") return;
    cancelling.current = true;
    dispatch({ type: "cancelAll" });
    const jobs = new Set<number>();
    for (const item of s.items) {
      if (item.jobId != null && (item.status === "queued" || item.status === "running")) jobs.add(item.jobId);
    }
    jobs.forEach((jobId) => void backend.cancelConversion(jobId));
  }, []);

  // --- PDFs opened with a password ------------------------------------------------
  // The backend keeps each password in memory while its file is in the list.
  const unlockedPaths = useRef(new Set<string>());
  const unlock = useCallback(async (key: number, password: string): Promise<ErrorPayload | null> => {
    const item = stateRef.current.items.find((i) => i.key === key);
    if (!item) return null;
    try {
      const file = await backend.unlockPdf(item.path, password);
      unlockedPaths.current.add(item.path);
      dispatch({ type: "unlocked", key, file });
      return null;
    } catch (error) {
      return toErrorPayload(error);
    }
  }, []);
  useEffect(() => {
    const listed = new Set(state.items.map((i) => i.path));
    for (const path of unlockedPaths.current) {
      if (listed.has(path)) continue;
      unlockedPaths.current.delete(path);
      void backend.forgetPdfPassword(path).catch(() => {});
    }
  }, [state.items]);

  const pickPicture = useCallback(
    () => backend.pickPicture(t("stamps.pickerTitle"), t("stamps.pickerFilter")).catch(() => null),
    [],
  );

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
    pickPicture,
    unlock,
    move: (key: number, delta: -1 | 1) => dispatch({ type: "move", key, delta }),
    selectTarget: (target: TargetFormat) => dispatch({ type: "selectTarget", target }),
    selectCompress: () => dispatch({ type: "selectCompress" }),
    setCompression: (compression: CompressOptions) => dispatch({ type: "setCompression", compression }),
    selectEditCopy: () => dispatch({ type: "selectEditCopy" }),
    setEdit: (edit: EditState) => dispatch({ type: "setEdit", edit }),
    selectPreset: (preset: QualityPreset) => dispatch({ type: "selectPreset", preset }),
    setAdvanced: (advanced: AdvancedOptions) => dispatch({ type: "setAdvanced", advanced }),
    setFrames: (frames: FrameOptions) => dispatch({ type: "setFrames", frames }),
    setRemoveLocation: (removeLocation: boolean) => dispatch({ type: "setRemoveLocation", removeLocation }),
    setPdf: (pdf: PdfOptions) => dispatch({ type: "setPdf", pdf }),
    setPages: (pages: EditorPage[] | null, cuts: string[]) => dispatch({ type: "setPages", pages, cuts }),
    start,
    cancelAll,
    back: () => dispatch({ type: "back" }),
    reset: () => dispatch({ type: "reset" }),
    openOutput: (jobId: number) => void backend.openOutput(jobId).catch(() => {}),
    revealOutput: (jobId: number) => void backend.revealOutput(jobId).catch(() => {}),
  };
}

export type QueueController = ReturnType<typeof useQueue>;
