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
import type { Preferences } from "../bindings/Preferences";
import type { Thumbnail } from "../bindings/Thumbnail";

export interface DropHandlers {
  /** A drag with files entered (`true`) or left (`false`) the window. */
  hover(active: boolean): void;
  drop(paths: string[]): void;
}

export interface Backend {
  platformInfo(): Promise<PlatformInfo>;
  appReady(): Promise<void>;
  engineInfo(): Promise<EngineInfo>;
  getPreferences(): Promise<Preferences>;
  setPreferences(preferences: Preferences): Promise<void>;
  pickFolder(title: string): Promise<string | null>;
  availableSpace(dir: string): Promise<number | null>;
  openReleasesPage(): Promise<void>;
  revealLog(): Promise<void>;
  clearLog(): Promise<void>;
  confirmClose(): Promise<void>;
  analyzeFile(path: string): Promise<FileAnalysis>;
  /** Opens a PDF protected by a password (kept in memory by the backend while the file is listed). */
  unlockPdf(path: string, password: string): Promise<FileAnalysis>;
  /** Forgets the password of a PDF that left the list. */
  forgetPdfPassword(path: string): Promise<void>;
  planConversion(path: string, media: MediaInfo, request: ConversionRequest): Promise<ConversionPlan>;
  /** Small pictures of some pages of a PDF (longest side `size` pixels), drawn locally. */
  pdfThumbnails(path: string, pages: number[], size: number): Promise<Thumbnail[]>;
  /** Native dialog choosing a PNG or JPEG picture (a watermark). */
  pickPicture(title: string, filterName: string): Promise<string | null>;
  /** A small picture of a photo (longest side `size` pixels at most) as a `data:` URL, drawn locally. */
  pictureThumbnail(path: string, size: number): Promise<string>;
  startConversion(path: string, request: ConversionRequest): Promise<number>;
  /** One job that turns every file, in order, into a single PDF. */
  startCombined(paths: string[], request: ConversionRequest): Promise<number>;
  cancelConversion(jobId: number): Promise<boolean>;
  openOutput(jobId: number): Promise<void>;
  revealOutput(jobId: number): Promise<void>;
  pickFiles(title: string, filterName: string): Promise<string[] | null>;
  /** Files Konvert was launched with ("Open with"); returned only once. */
  takeLaunchFiles(): Promise<string[]>;
  openLicenses(): Promise<void>;
  /** Files opened with Konvert while it is already running. */
  onOpenFiles(handler: (paths: string[]) => void): Promise<() => void>;
  onJobUpdate(handler: (event: JobEvent) => void): Promise<() => void>;
  onFileDrop(handlers: DropHandlers): Promise<() => void>;
  /** The window close button was pressed while conversions are running. */
  onCloseRequested(handler: () => void): Promise<() => void>;
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
  getPreferences: () => call("get_preferences"),
  setPreferences: (preferences) => call("set_preferences", { preferences }),
  pickFolder: (title) => call("pick_folder", { title }),
  availableSpace: (dir) => call("available_space", { dir }),
  openReleasesPage: () => call("open_releases_page"),
  revealLog: () => call("reveal_log"),
  clearLog: () => call("clear_log"),
  confirmClose: () => call("confirm_close"),
  analyzeFile: (path) => call("analyze_file", { path }),
  unlockPdf: (path, password) => call("unlock_pdf", { path, password }),
  forgetPdfPassword: (path) => call("forget_pdf_password", { path }),
  planConversion: (path, media, request) => call("plan_conversion", { path, media, request }),
  pdfThumbnails: (path, pages, size) => call("pdf_thumbnails", { path, pages, size }),
  pictureThumbnail: (path, size) => call("picture_thumbnail", { path, size }),
  pickPicture: (title, filterName) => call("pick_picture", { title, filterName }),
  startConversion: (path, request) => call("start_conversion", { path, request }),
  startCombined: (paths, request) => call("start_combined", { paths, request }),
  cancelConversion: (jobId) => call("cancel_conversion", { jobId }),
  openOutput: (jobId) => call("open_output", { jobId }),
  revealOutput: (jobId) => call("reveal_output", { jobId }),
  pickFiles: (title, filterName) => call("pick_files", { title, filterName }),
  takeLaunchFiles: () => call("take_launch_files"),
  openLicenses: () => call("open_licenses"),
  onOpenFiles: (handler) => listen<string[]>("open-files", (e) => handler(e.payload)),
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
  onCloseRequested: (handler) => listen("close-requested", () => handler()),
};

const insideTauri = "__TAURI_INTERNALS__" in window;

// In `npm run dev` opened in a plain browser (design work), a simulated backend
// replaces Tauri. The branch is removed from production builds.
export const backend: Backend =
  insideTauri || !import.meta.env.DEV ? tauriBackend : (await import("./mock")).mockBackend;
