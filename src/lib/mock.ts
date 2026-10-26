// Development-only simulated backend, used when the UI runs in a plain browser
// (`npm run dev` without Tauri) to work on the design. Never bundled in releases.
// URL parameters: `?mock=error` simulates a damaged file; `?files=a.wav|b.mp4`
// sets what the file picker returns.
import type { Backend } from "./api";
import type { AudioCodecInfo } from "../bindings/AudioCodecInfo";
import type { ConversionPlan } from "../bindings/ConversionPlan";
import type { ConversionWarning } from "../bindings/ConversionWarning";
import type { FileAnalysis } from "../bindings/FileAnalysis";
import type { JobEvent } from "../bindings/JobEvent";
import type { Preferences } from "../bindings/Preferences";
import type { TargetFormat } from "../bindings/TargetFormat";
import type { TargetOption } from "../bindings/TargetOption";

const params = new URLSearchParams(location.search);
const listeners = new Set<(e: JobEvent) => void>();
const timers = new Map<number, number>();
let nextJob = 1;

let preferences: Preferences = {
  language: "auto",
  theme: "auto",
  defaultPreset: "balanced",
  outputDir: null,
  debugLogging: false,
};

const audioCodec = (codec: AudioCodecInfo["codec"]): AudioCodecInfo => ({
  codec,
  lossless: codec === "flac" || codec === "pcm",
  sampleRates: codec === "opus" ? [8000, 12000, 16000, 24000, 48000] : null,
  maxChannels: codec === "mp3" ? 2 : null,
});

const option = (format: TargetFormat, label: string, group: TargetOption["group"], lossless = false): TargetOption => ({
  format,
  label,
  extension: format,
  group,
  lossless,
  sameAsInput: false,
  kind: group === "video" ? "video" : "audio",
  videoCodecs:
    group !== "video" ? [] : format === "webm" ? ["vp9", "av1"] : format === "gif" ? ["gif"] : ["h264", "av1"],
  audioCodecs:
    format === "gif"
      ? []
      : [audioCodec(lossless ? "flac" : format === "webm" || format === "opus" ? "opus" : "aac")],
});

const AUDIO: TargetOption[] = [
  option("mp3", "MP3", "audio"),
  option("m4a", "M4A (AAC)", "audio"),
  option("flac", "FLAC", "audio", true),
  option("ogg", "OGG (Vorbis)", "audio"),
  option("opus", "OPUS", "audio"),
  option("wma", "WMA", "audio"),
  option("aiff", "AIFF", "audio", true),
];

function analysis(path: string): FileAnalysis {
  const name = path.split(/[\\/]/).pop() ?? path;
  const isVideo = /\.(mp4|mov|mkv|webm|avi)$/i.test(name);
  if (isVideo) {
    return {
      path,
      name,
      media: {
        container: "mov,mp4,m4a,3gp,3g2,mj2",
        kind: "videoWithAudio",
        durationSecs: 754,
        sizeBytes: 486_000_000,
        bitRate: 5_150_000,
        video: [{ index: 0, codec: "h264", width: 1920, height: 1080, fps: 29.97, bitRate: 4_960_000, pixFmt: "yuv420p" }],
        audio: [{ index: 1, codec: "aac", sampleRate: 48000, channels: 2, bitRate: 192_000, bitDepth: null }],
        coverArt: null,
        subtitleStreams: 0,
      },
      targets: [
        ...(["mp4", "mkv", "mov", "webm", "avi", "gif"] as TargetFormat[]).map((f) => ({
          ...option(f, f === "webm" ? "WebM" : f.toUpperCase(), "video"),
          sameAsInput: f === "mp4",
        })),
        ...AUDIO.map((o) => ({ ...o, group: "audioOnly" as const })),
        option("wav", "WAV", "audioOnly", true),
      ],
    };
  }
  return {
    path,
    name,
    media: {
      container: "wav",
      kind: "audio",
      durationSecs: 215.4,
      sizeBytes: 38_000_000,
      bitRate: 1_411_200,
      video: [],
      audio: [{ index: 0, codec: "pcm_s16le", sampleRate: 44100, channels: 2, bitRate: 1_411_200, bitDepth: 16 }],
      coverArt: null,
      subtitleStreams: 0,
    },
    targets: AUDIO,
  };
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const emit = (e: JobEvent) => listeners.forEach((l) => l(e));

export const mockBackend: Backend = {
  async platformInfo() {
    return { vibrancy: false };
  },
  async appReady() {},
  async engineInfo() {
    return { appVersion: "0.1.0-dev", ffmpegVersion: "ffmpeg version n9.0 (simulated)" };
  },
  async getPreferences() {
    return preferences;
  },
  async setPreferences(next) {
    preferences = next;
  },
  async pickFolder() {
    return "D:\\Conversioni";
  },
  async availableSpace() {
    return 120_000_000_000;
  },
  async openReleasesPage() {},
  async revealLog() {},
  async clearLog() {},
  async confirmClose() {},
  async analyzeFile(path) {
    await sleep(600);
    if (params.get("mock") === "error") {
      throw { code: "corrupt_input", details: "x.wav: Invalid data found when processing input" };
    }
    return analysis(path);
  },
  async planConversion(_path, media, request) {
    const video = request.target === "mp4" || request.target === "mkv" || request.target === "mov";
    const lossless = request.target === "flac" || request.target === "aiff" || request.target === "wav";
    const warnings: ConversionWarning[] = [];
    if (media.kind === "audio" && !lossless) warnings.push("losslessToLossy");
    if (request.target === "gif") warnings.push("gifLimitations", "audioDropped");
    if (request.target === "webm" || request.target === "avi") warnings.push("lossyReencode");
    const remux = video && request.preset !== "small" && request.target !== "mp4";
    const factor = { max: 1.4, balanced: 1, small: 0.55 }[request.preset];
    const plan: ConversionPlan = {
      target: request.target,
      videoStream: media.video[0]?.index ?? null,
      video: remux ? { mode: "copy", codec: "h264" } : null,
      audioStream: media.audio[0]?.index ?? null,
      audio: remux ? { mode: "copy", codec: "aac" } : null,
      coverArtStream: null,
      warnings,
      estimate: {
        bytes: Math.round(
          (media.sizeBytes ?? 0) * (lossless ? 0.6 : media.kind === "audio" ? 0.14 * factor : factor * 0.8),
        ),
        approximate: !lossless,
      },
    };
    return plan;
  },
  async startConversion(path, request) {
    const jobId = nextJob++;
    const name = (path.split(/[\\/]/).pop() ?? "file").replace(/\.\w+$/, "");
    let fraction = 0;
    const delay = (jobId - 1) * 2500;
    setTimeout(() => emit({ jobId, update: { state: "running" } }), 100 + delay);
    setTimeout(() => {
      const timer = window.setInterval(() => {
        fraction = Math.min(1, fraction + 0.04);
        emit({
          jobId,
          update: {
            state: "progress",
            progress: { fraction, etaSecs: (1 - fraction) * 25, speed: 8.4, processedSecs: null, outputBytes: null },
          },
        });
        if (fraction >= 1) {
          clearInterval(timer);
          emit({
            jobId,
            update: {
              state: "done",
              outcome: { output: `C:\\Musica\\${name}.${request.target}`, sizeBytes: 5_400_000, elapsed: 6.2 },
            },
          });
        }
      }, 100);
      timers.set(jobId, timer);
    }, 100 + delay);
    return jobId;
  },
  async cancelConversion(jobId) {
    clearInterval(timers.get(jobId));
    setTimeout(() => emit({ jobId, update: { state: "cancelled" } }), 300);
    return true;
  },
  async openOutput() {},
  async revealOutput() {},
  async pickFiles() {
    await sleep(200);
    return params.get("files")?.split("|") ?? [params.get("file") ?? "C:\\Musica\\Brano di prova.wav"];
  },
  async onJobUpdate(handler) {
    listeners.add(handler);
    return () => listeners.delete(handler);
  },
  async onFileDrop() {
    return () => {};
  },
  async onCloseRequested() {
    return () => {};
  },
  async takeLaunchFiles() {
    return [];
  },
  async openLicenses() {},
  async onOpenFiles() {
    return () => {};
  },
};
