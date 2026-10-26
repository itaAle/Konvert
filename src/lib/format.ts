// Human-friendly formatting of sizes, durations and media details.
import { intlLocale, t } from "../i18n";
import type { MediaInfo } from "../bindings/MediaInfo";

const number = (value: number, maxFractionDigits = 1) =>
  new Intl.NumberFormat(intlLocale(), { maximumFractionDigits: maxFractionDigits }).format(value);

/** 1536 → "1,5 KB" (decimal units, like Finder and Explorer's "size on disk" UI). */
export function formatBytes(bytes: number): string {
  if (bytes < 1000) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = -1;
  do {
    value /= 1000;
    unit++;
  } while (value >= 1000 && unit < units.length - 1);
  return `${number(value, value < 10 ? 1 : 0)} ${units[unit]}`;
}

/** 75 → "1:15", 3725 → "1:02:05". */
export function formatDuration(seconds: number): string {
  const total = Math.max(0, Math.round(seconds));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

/** Remaining time in words: "pochi secondi", "40 s", "3 min", "1 h 5 min". */
export function formatRemaining(seconds: number): string {
  if (seconds < 5) return t("time.fewSeconds");
  if (seconds < 60) return t("time.seconds", { n: Math.round(seconds / 5) * 5 });
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return t("time.minutes", { n: minutes });
  return t("time.hoursMinutes", { h: Math.floor(minutes / 60), m: minutes % 60 });
}

/** 2.345 → "2,3". */
export function formatSpeed(speed: number): string {
  return number(speed, speed < 10 ? 1 : 0);
}

/** 44100 → "44,1 kHz". */
export function formatSampleRate(hz: number): string {
  return `${number(hz / 1000)} kHz`;
}

export function formatChannels(channels: number): string {
  if (channels === 1) return t("file.channels.mono");
  if (channels === 2) return t("file.channels.stereo");
  return t("file.channels.many", { count: channels });
}

/** Short codec names for display. */
const CODEC_NAMES: Record<string, string> = {
  h264: "H.264",
  hevc: "HEVC",
  av1: "AV1",
  vp9: "VP9",
  vp8: "VP8",
  mpeg4: "MPEG-4",
  prores: "ProRes",
  mp3: "MP3",
  aac: "AAC",
  opus: "Opus",
  vorbis: "Vorbis",
  flac: "FLAC",
  alac: "ALAC",
  wmav2: "WMA",
  ac3: "AC-3",
  eac3: "E-AC-3",
  gif: "GIF",
};

export function codecName(codec: string): string {
  if (codec.startsWith("pcm_")) return "PCM";
  return CODEC_NAMES[codec] ?? codec.toUpperCase();
}

/** Picture formats by codec (HEIC and AVIF files hold HEVC and AV1 pictures). */
const PICTURE_NAMES: Record<string, string> = {
  mjpeg: "JPEG",
  png: "PNG",
  webp: "WebP",
  av1: "AVIF",
  hevc: "HEIC",
  bmp: "BMP",
  tiff: "TIFF",
  jpegxl: "JPEG XL",
  jpeg2000: "JPEG 2000",
};

/** One-line technical summary, e.g. "1920×1080 · 30 fps · H.264 · AAC stereo". */
export function describeMedia(media: MediaInfo): string {
  const parts: string[] = [];
  const video = media.video[0];
  const audio = media.audio[0];
  if (media.kind === "image") {
    if (video?.width && video.height) parts.push(`${video.width}×${video.height}`);
    if (video) parts.push(PICTURE_NAMES[video.codec] ?? codecName(video.codec));
    return parts.join(" · ");
  }
  if (video) {
    if (video.width && video.height) parts.push(`${video.width}×${video.height}`);
    if (video.fps) parts.push(`${number(video.fps, 2)} fps`);
    parts.push(codecName(video.codec));
  }
  if (audio) {
    const details = [codecName(audio.codec)];
    if (!video && audio.sampleRate) details.push(formatSampleRate(audio.sampleRate));
    if (audio.channels) details.push(formatChannels(audio.channels).toLowerCase());
    if (!video && audio.bitDepth && audio.codec !== "mp3") details.push(`${audio.bitDepth} bit`);
    parts.push(video ? details.join(" ") : details.join(" · "));
  } else if (video) {
    parts.push(t("file.noAudio"));
  }
  return parts.join(" · ");
}
