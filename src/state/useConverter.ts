// Connects the converter state machine to the backend.
import { useCallback, useEffect, useReducer, useRef, useState } from "react";

import { backend, toErrorPayload } from "../lib/api";
import type { JobEvent } from "../bindings/JobEvent";
import type { QualityPreset } from "../bindings/QualityPreset";
import type { TargetFormat } from "../bindings/TargetFormat";
import { t } from "../i18n";
import { converterReducer, initialState, requestFor } from "./converter";

export function useConverter() {
  const [state, dispatch] = useReducer(converterReducer, initialState);
  const [dragging, setDragging] = useState(false);

  const stateRef = useRef(state);
  stateRef.current = state;
  // Job events can arrive before `start_conversion` returns the job id
  // (a fast remux may even finish first): keep them until the id is known.
  const pendingEvents = useRef<JobEvent[]>([]);

  const analyze = useCallback((path: string) => {
    if (stateRef.current.phase === "converting") return;
    dispatch({ type: "analyze", path });
    backend.analyzeFile(path).then(
      (file) => dispatch({ type: "analyzed", file }),
      (error) => dispatch({ type: "analyzeFailed", error: toErrorPayload(error) }),
    );
  }, []);

  // Backend events: job updates and files dropped on the window.
  useEffect(() => {
    const unlisten = [
      backend.onJobUpdate((event) => {
        const s = stateRef.current;
        if (s.phase === "converting" && s.jobId === null) pendingEvents.current.push(event);
        else dispatch({ type: "job", event });
      }),
      backend.onFileDrop({
        hover: setDragging,
        // One file at a time for now; batch conversion comes with the queue view.
        drop: (paths) => analyze(paths[0]),
      }),
    ];
    return () => {
      unlisten.forEach((p) => p.then((stop) => stop()));
    };
  }, [analyze]);

  // Plan the conversion whenever the selection changes.
  const sel = state.phase === "ready" ? state.sel : null;
  useEffect(() => {
    if (!sel || !sel.target || sel.plan || sel.planError) return;
    const request = requestFor(sel);
    if (!request) return;
    const { target, preset } = request;
    backend.planConversion(sel.file.path, sel.file.media, request).then(
      (plan) => dispatch({ type: "planned", target, preset, plan }),
      (error) => dispatch({ type: "planFailed", target, preset, error: toErrorPayload(error) }),
    );
  }, [sel]);

  const chooseFiles = useCallback(async () => {
    const paths = await backend.pickFiles(t("drop.pickerTitle"), t("drop.pickerFilter"));
    if (paths && paths.length > 0) analyze(paths[0]);
  }, [analyze]);

  const start = useCallback(() => {
    const s = stateRef.current;
    if (s.phase !== "ready") return;
    const request = requestFor(s.sel);
    if (!request || !s.sel.plan) return;
    pendingEvents.current = [];
    dispatch({ type: "start" });
    backend.startConversion(s.sel.file.path, request).then(
      (jobId) => {
        dispatch({ type: "started", jobId });
        const early = pendingEvents.current.filter((e) => e.jobId === jobId);
        pendingEvents.current = [];
        early.forEach((event) => dispatch({ type: "job", event }));
      },
      (error) => dispatch({ type: "startFailed", error: toErrorPayload(error) }),
    );
  }, []);

  const cancel = useCallback(() => {
    const s = stateRef.current;
    if (s.phase !== "converting" || s.jobId === null) return;
    dispatch({ type: "cancel" });
    void backend.cancelConversion(s.jobId);
  }, []);

  return {
    state,
    dragging,
    analyze,
    chooseFiles,
    selectTarget: (target: TargetFormat) => dispatch({ type: "selectTarget", target }),
    selectPreset: (preset: QualityPreset) => dispatch({ type: "selectPreset", preset }),
    start,
    cancel,
    back: () => dispatch({ type: "back" }),
    reset: () => dispatch({ type: "reset" }),
    openOutput: (jobId: number) => void backend.openOutput(jobId).catch(() => {}),
    revealOutput: (jobId: number) => void backend.revealOutput(jobId).catch(() => {}),
  };
}
