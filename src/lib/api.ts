// Typed access to the Rust backend. Every call is a local IPC message to the
// Konvert process: nothing here uses the network.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";

import type { ConversionPlan } from "../bindings/ConversionPlan";
import type { ConversionRequest } from "../bindings/ConversionRequest";
import type { EngineInfo } from "../bindings/EngineInfo";
import type { ErrorPayload } from "../bindings/ErrorPayload";
import type { FileAnalysis } from "../bindings/FileAnalysis";
import type { JobEvent } from "../bindings/JobEvent";
import type { MediaInfo } from "../bindings/MediaInfo";
import type { PlatformInfo } from "../bindings/PlatformInfo";

export interface DropHandlers {
  /** A drag with files entered (`true`) or left (`false`) the window. */
  hover(active: boolean): void;
  drop(paths: string[]): void;
}

export interface Backend {
  platformInfo(): Promise<PlatformInfo>;
  appReady(): Promise<void>;
  engineInfo(): Promise<EngineInfo>;
  analyzeFile(path: string): Promise<FileAnalysis>;
  planConversion(path: string, media: MediaInfo, request: ConversionRequest): Promise<ConversionPlan>;
  startConversion(path: string, request: ConversionRequest, outputDir?: string | null): Promise<number>;
  cancelConversion(jobId: number): Promise<boolean>;
  openOutput(jobId: number): Promise<void>;
  revealOutput(jobId: number): Promise<void>;
  pickFiles(title: string, filterName: string): Promise<string[] | null>;
  onJobUpdate(handler: (event: JobEvent) => void): Promise<() => void>;
  onFileDrop(handlers: DropHandlers): Promise<() => void>;
}

/** Normalizes any rejection into the backend's `{ code, details }` shape. */
export function toErrorPayload(error: unknown): ErrorPayload {
  if (typeof error === "object" && error !== null && "code" in error) {
    const { code, details } = error as { code: unknown; details?: unknown };
    return { code: String(code), details: details == null ? null : String(details) };
  }
  return { code: "unknown", details: error == null ? null : String(error) };
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toErrorPayload(error);
  }
}

const tauriBackend: Backend = {
  platformInfo: () => call("platform_info"),
  appReady: () => call("app_ready"),
  engineInfo: () => call("engine_info"),
  analyzeFile: (path) => call("analyze_file", { path }),
  planConversion: (path, media, request) => call("plan_conversion", { path, media, request }),
  startConversion: (path, request, outputDir = null) =>
    call("start_conversion", { path, request, outputDir }),
  cancelConversion: (jobId) => call("cancel_conversion", { jobId }),
  openOutput: (jobId) => call("open_output", { jobId }),
  revealOutput: (jobId) => call("reveal_output", { jobId }),
  pickFiles: (title, filterName) => call("pick_files", { title, filterName }),
  onJobUpdate: (handler) => listen<JobEvent>("job-update", (e) => handler(e.payload)),
  onFileDrop: (handlers) =>
    getCurrentWebview().onDragDropEvent(({ payload }) => {
      switch (payload.type) {
        case "enter":
          handlers.hover(payload.paths.length > 0);
          break;
        case "leave":
          handlers.hover(false);
          break;
        case "drop":
          handlers.hover(false);
          if (payload.paths.length > 0) handlers.drop(payload.paths);
          break;
      }
    }),
};

const insideTauri = "__TAURI_INTERNALS__" in window;

// In `npm run dev` opened in a plain browser (design work), a simulated backend
// replaces Tauri. The branch is removed from production builds.
export const backend: Backend =
  insideTauri || !import.meta.env.DEV ? tauriBackend : (await import("./mock")).mockBackend;
