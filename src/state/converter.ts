// State machine of the single-file conversion flow:
// empty → analyzing → ready → converting → done | error (and back).
// Pure reducer: side effects live in `useConverter`.
import type { ConversionOutcome } from "../bindings/ConversionOutcome";
import type { ConversionPlan } from "../bindings/ConversionPlan";
import type { ConversionRequest } from "../bindings/ConversionRequest";
import type { ErrorPayload } from "../bindings/ErrorPayload";
import type { FileAnalysis } from "../bindings/FileAnalysis";
import type { JobEvent } from "../bindings/JobEvent";
import type { Progress } from "../bindings/Progress";
import type { QualityPreset } from "../bindings/QualityPreset";
import type { TargetFormat } from "../bindings/TargetFormat";

/** The file and the user's choices for it. */
export interface Selection {
  file: FileAnalysis;
  target: TargetFormat | null;
  preset: QualityPreset;
  /** Plan for the current target/preset (`null` while computing). */
  plan: ConversionPlan | null;
  planError: ErrorPayload | null;
}

export type Notice = "cancelled";

export type ConverterState =
  | { phase: "empty"; notice: Notice | null }
  | { phase: "analyzing"; path: string }
  | { phase: "ready"; sel: Selection; notice: Notice | null }
  | {
      phase: "converting";
      sel: Selection;
      jobId: number | null;
      status: "starting" | "queued" | "running" | "cancelling";
      progress: Progress | null;
    }
  | { phase: "done"; sel: Selection; jobId: number; outcome: ConversionOutcome }
  | { phase: "error"; error: ErrorPayload; sel: Selection | null };

export type ConverterAction =
  | { type: "analyze"; path: string }
  | { type: "analyzed"; file: FileAnalysis }
  | { type: "analyzeFailed"; error: ErrorPayload }
  | { type: "selectTarget"; target: TargetFormat }
  | { type: "selectPreset"; preset: QualityPreset }
  | { type: "planned"; target: TargetFormat; preset: QualityPreset; plan: ConversionPlan }
  | { type: "planFailed"; target: TargetFormat; preset: QualityPreset; error: ErrorPayload }
  | { type: "start" }
  | { type: "started"; jobId: number }
  | { type: "startFailed"; error: ErrorPayload }
  | { type: "job"; event: JobEvent }
  | { type: "cancel" }
  | { type: "back" }
  | { type: "reset" };

export const initialState: ConverterState = { phase: "empty", notice: null };

/** The request sent to the backend for a selection. */
export function requestFor(sel: Selection): ConversionRequest | null {
  return sel.target ? { target: sel.target, preset: sel.preset, advanced: {} } : null;
}

export function converterReducer(state: ConverterState, action: ConverterAction): ConverterState {
  switch (action.type) {
    case "analyze":
      // Ignore drops while converting.
      if (state.phase === "converting") return state;
      return { phase: "analyzing", path: action.path };

    case "analyzed":
      if (state.phase !== "analyzing") return state;
      return {
        phase: "ready",
        notice: null,
        sel: { file: action.file, target: null, preset: "balanced", plan: null, planError: null },
      };

    case "analyzeFailed":
      if (state.phase !== "analyzing") return state;
      return { phase: "error", error: action.error, sel: null };

    case "selectTarget":
    case "selectPreset": {
      if (state.phase !== "ready") return state;
      const sel = { ...state.sel, plan: null, planError: null };
      if (action.type === "selectTarget") sel.target = action.target;
      else sel.preset = action.preset;
      return { phase: "ready", notice: null, sel };
    }

    case "planned":
    case "planFailed": {
      if (state.phase !== "ready") return state;
      const { sel } = state;
      // Drop answers for a selection that has changed in the meantime.
      if (sel.target !== action.target || sel.preset !== action.preset) return state;
      return action.type === "planned"
        ? { ...state, sel: { ...sel, plan: action.plan, planError: null } }
        : { ...state, sel: { ...sel, plan: null, planError: action.error } };
    }

    case "start":
      if (state.phase !== "ready" || !state.sel.target || !state.sel.plan) return state;
      return { phase: "converting", sel: state.sel, jobId: null, status: "starting", progress: null };

    case "started":
      if (state.phase !== "converting" || state.jobId !== null) return state;
      return { ...state, jobId: action.jobId, status: state.status === "starting" ? "queued" : state.status };

    case "startFailed":
      if (state.phase !== "converting") return state;
      return { phase: "error", error: action.error, sel: state.sel };

    case "job": {
      if (state.phase !== "converting" || state.jobId !== action.event.jobId) return state;
      const update = action.event.update;
      switch (update.state) {
        case "queued":
          return state;
        case "running":
          return state.status === "cancelling" ? state : { ...state, status: "running" };
        case "progress":
          return { ...state, progress: update.progress };
        case "done":
          return { phase: "done", sel: state.sel, jobId: state.jobId, outcome: update.outcome };
        case "failed":
          return { phase: "error", error: update.error, sel: state.sel };
        case "cancelled":
          return { phase: "ready", sel: state.sel, notice: "cancelled" };
      }
      return state;
    }

    case "cancel":
      if (state.phase !== "converting" || state.jobId === null) return state;
      return { ...state, status: "cancelling" };

    case "back":
      if ((state.phase === "error" || state.phase === "done") && state.sel) {
        return { phase: "ready", sel: state.sel, notice: null };
      }
      return initialState;

    case "reset":
      return state.phase === "converting" ? state : initialState;
  }
}
