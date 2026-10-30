// Development-only simulated backend, used when the UI runs in a plain browser
// (`npm run dev` without Tauri) to work on the design. Never bundled in releases.
// URL parameters: `?mock=error` simulates a damaged file; `?files=a.wav|b.mp4`
// sets what the file picker returns (a `.pdf` is a 12-page document).
import type { Backend } from "./api";
import type { AudioCodecInfo } from "../bindings/AudioCodecInfo";
import type { ConversionPlan } from "../bindings/ConversionPlan";
import type { ConversionWarning } from "../bindings/ConversionWarning";
import type { FileAnalysis } from "../bindings/FileAnalysis";
import type { JobEvent } from "../bindings/JobEvent";
import type { PdfOptions } from "../bindings/PdfOptions";
import type { PictureEdit } from "../bindings/PictureEdit";
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

const ANIMATIONS: TargetFormat[] = ["gif", "animatedWebp", "apng"];
const EXTENSIONS: Partial<Record<TargetFormat, string>> = {
  alac: "m4a",
  animatedWebp: "webp",
  stillGif: "gif",
};

const option = (
  format: TargetFormat,
  label: string,
  group: TargetOption["group"],
  lossless = false,
  secondary = false,
): TargetOption => ({
  format,
  label,
  extension: EXTENSIONS[format] ?? format,
  group,
  lossless,
  sameAsInput: false,
  kind:
    group === "video"
      ? "video"
      : group === "document" || group === "text"
        ? "document"
        : ["image", "frames", "pages"].includes(group)
          ? "image"
          : "audio",
  secondary,
  videoCodecs:
    group !== "video"
      ? []
      : format === "webm"
        ? ["vp9", "av1"]
        : format === "gif"
          ? ["gif"]
          : format === "animatedWebp"
            ? ["webp"]
            : format === "apng"
              ? ["apng"]
              : format === "mpg"
                ? ["mpeg2"]
                : ["h264", "av1"],
  audioCodecs:
    ANIMATIONS.includes(format) || ["image", "frames", "pages", "document", "text"].includes(group)
      ? []
      : [audioCodec(lossless ? "flac" : format === "webm" || format === "opus" ? "opus" : "aac")],
});

const AUDIO: TargetOption[] = [
  option("mp3", "MP3", "audio"),
  option("m4a", "M4A (AAC)", "audio"),
  option("alac", "ALAC (M4A)", "audio", true),
  option("flac", "FLAC", "audio", true),
  option("ogg", "OGG (Vorbis)", "audio"),
  option("opus", "OPUS", "audio"),
  option("wma", "WMA", "audio"),
  option("aiff", "AIFF", "audio", true),
  option("aac", "AAC", "audio", false, true),
  option("ac3", "AC3", "audio", false, true),
  option("eac3", "E-AC3", "audio", false, true),
  option("mp2", "MP2", "audio", false, true),
  option("caf", "CAF (ALAC)", "audio", true, true),
  option("mka", "MKA", "audio", false, true),
];

const VIDEO: TargetOption[] = [
  option("mp4", "MP4", "video"),
  option("mkv", "MKV", "video"),
  option("mov", "MOV", "video"),
  option("webm", "WebM", "video"),
  option("avi", "AVI", "video"),
  option("ts", "TS", "video", false, true),
  option("mpg", "MPG (MPEG-2)", "video", false, true),
  option("3gp", "3GP", "video", false, true),
  option("flv", "FLV", "video", false, true),
  option("gif", "GIF", "video"),
  option("animatedWebp", "WebP", "video"),
  option("apng", "APNG", "video", true, true),
];

const IMAGES: TargetOption[] = [
  option("jpg", "JPG", "image"),
  option("png", "PNG", "image", true),
  option("webp", "WebP", "image"),
  option("avif", "AVIF", "image"),
  option("bmp", "BMP", "image", true, true),
  option("tiff", "TIFF", "image", true, true),
  option("stillGif", "GIF", "image", false, true),
  option("ico", "ICO", "image", true, true),
  option("jxl", "JPEG XL", "image", false, true),
];

const FRAMES: TargetOption[] = [
  option("jpg", "JPG", "frames"),
  option("png", "PNG", "frames", true),
  option("webp", "WebP", "frames"),
];

const PDF = option("pdf", "PDF", "document");
const DOCX = option("docx", "DOCX (Word)", "text");
const TXT = option("txt", "TXT", "text");

const PAGES: TargetOption[] = [
  option("png", "PNG", "pages", true),
  option("jpg", "JPG", "pages"),
  option("webp", "WebP", "pages"),
];

const MOCK_PAGES = 12;

/** Same rules as the core: "1-3, 5, 8-", empty for every page. */
function parsePages(text: string | null | undefined, count: number): number[] {
  const parts = (text ?? "")
    .split(/[,;]/)
    .map((p) => p.trim())
    .filter(Boolean);
  if (parts.length === 0) return Array.from({ length: count }, (_, i) => i + 1);
  const chosen = new Set<number>();
  for (const part of parts) {
    const m = part.match(/^(\d*)\s*(-?)\s*(\d*)$/);
    const first = m?.[1] ? Number(m[1]) : m?.[2] ? 1 : NaN;
    const last = m?.[3] ? Number(m[3]) : m?.[2] ? count : first;
    if (!(first >= 1 && first <= last && last <= count)) {
      throw { code: "invalid_pages", details: `invalid page range "${part}" (the document has ${count} pages)` };
    }
    for (let p = first; p <= last; p++) chosen.add(p);
  }
  return [...chosen].sort((a, b) => a - b);
}

/** Same rules as the core: pages in each new document (empty: one document). */
function splitSizes(pdf: PdfOptions | null | undefined, pages: number): number[] {
  let sizes: number[] = [];
  if (pdf?.split === "every") {
    for (let start = 0; start < pages; start += pdf.splitEvery) sizes.push(Math.min(pdf.splitEvery, pages - start));
  } else if (pdf?.split === "after") {
    const cuts = [...new Set(pdf.splitAfter.filter((c) => c >= 1 && c < pages))].sort((a, b) => a - b);
    sizes = [...cuts, pages].map((cut, i, all) => cut - (i > 0 ? all[i - 1] : 0));
  }
  return sizes.length > 1 ? sizes : [];
}

/** Size of an edited picture, by the core's rules (without the format limits). */
function editedPicture(width: number, height: number, edit: PictureEdit | null) {
  const turned = (edit?.rotation ?? 0) % 180 === 90;
  let w = turned ? height : width;
  let h = turned ? width : height;
  if (edit?.crop) {
    w = Math.max(1, edit.crop.width * w);
    h = Math.max(1, edit.crop.height * h);
  }
  const resize = edit?.resize;
  const cap = (f: number) => (edit?.enlarge ? f : Math.min(f, 1));
  let stretched = false;
  if (resize?.mode === "percent") [w, h] = [w * cap(resize.percent / 100), h * cap(resize.percent / 100)];
  else if (resize?.mode === "longSide") {
    const f = cap(resize.side / Math.max(w, h));
    [w, h] = [w * f, h * f];
  } else if (resize?.mode === "pixels" && resize.keepRatio) {
    const f = cap(Math.min(resize.width != null ? resize.width / w : Infinity, resize.height != null ? resize.height / h : Infinity));
    [w, h] = [w * f, h * f];
  } else if (resize?.mode === "pixels") {
    const nw = resize.width == null ? w : edit?.enlarge ? resize.width : Math.min(resize.width, w);
    const nh = resize.height == null ? h : edit?.enlarge ? resize.height : Math.min(resize.height, h);
    stretched = Math.abs(nw / nh / (w / h) - 1) > 0.01;
    [w, h] = [nw, nh];
  }
  return { width: Math.max(1, Math.round(w)), height: Math.max(1, Math.round(h)), stretched };
}

/** A page drawn on a canvas: a title, lines of "text" and the page number. */
function mockPage(number: number, widthPt: number, heightPt: number, size: number): string {
  const scale = size / Math.max(widthPt, heightPt);
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(widthPt * scale);
  canvas.height = Math.round(heightPt * scale);
  const g = canvas.getContext("2d")!;
  const u = canvas.width / 20;
  g.fillStyle = "#fff";
  g.fillRect(0, 0, canvas.width, canvas.height);
  g.fillStyle = `hsl(${(number * 47) % 360} 55% 55%)`;
  g.fillRect(u * 2, u * 2, canvas.width - u * 4, u * 2.2);
  g.fillStyle = "#c9ccd1";
  for (let y = u * 6; y < canvas.height - u * 5; y += u * 1.3) {
    g.fillRect(u * 2, y, (canvas.width - u * 4) * (0.6 + ((y * number) % 7) / 18), u * 0.5);
  }
  g.fillStyle = "#6b7280";
  g.font = `600 ${Math.round(u * 1.6)}px sans-serif`;
  g.textAlign = "center";
  g.fillText(String(number), canvas.width / 2, canvas.height - u * 1.8);
  return canvas.toDataURL("image/png");
}

/** PDFs opened with their password in this session. */
const mockUnlocked = new Set<string>();

/** A photo for the page editor: sky, sun and hills, in the photo's proportions. */
function mockPhoto(path: string, width: number, height: number, size: number): string {
  const scale = size / Math.max(width, height);
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(width * scale);
  canvas.height = Math.round(height * scale);
  const g = canvas.getContext("2d")!;
  const hue = [...path].reduce((h, c) => h + c.charCodeAt(0), 0) % 360;
  const sky = g.createLinearGradient(0, 0, 0, canvas.height);
  sky.addColorStop(0, `hsl(${hue} 70% 70%)`);
  sky.addColorStop(1, `hsl(${(hue + 40) % 360} 60% 88%)`);
  g.fillStyle = sky;
  g.fillRect(0, 0, canvas.width, canvas.height);
  g.fillStyle = "#ffd60a";
  g.beginPath();
  g.arc(canvas.width * 0.72, canvas.height * 0.3, canvas.width * 0.09, 0, Math.PI * 2);
  g.fill();
  g.fillStyle = `hsl(${(hue + 120) % 360} 35% 38%)`;
  g.beginPath();
  g.moveTo(0, canvas.height);
  g.quadraticCurveTo(canvas.width * 0.3, canvas.height * 0.45, canvas.width * 0.6, canvas.height * 0.8);
  g.quadraticCurveTo(canvas.width * 0.85, canvas.height * 0.6, canvas.width, canvas.height * 0.75);
  g.lineTo(canvas.width, canvas.height);
  g.fill();
  return canvas.toDataURL("image/png");
}

function analysis(path: string): FileAnalysis {
  const name = path.split(/[\\/]/).pop() ?? path;
  if (/\.pdf$/i.test(name)) {
    return {
      path,
      name,
      media: {
        container: "pdf",
        kind: "document",
        durationSecs: null,
        sizeBytes: 1_240_000,
        bitRate: null,
        video: [],
        audio: [],
        coverArt: null,
        subtitleStreams: 0,
        tracks: [],
        hasTags: false,
        picture: null,
        hasLocation: false,
        document: { pages: MOCK_PAGES, widthPt: 595.3, heightPt: 841.9, bookmarkPages: [1, 3, 9], form: "fillable", locked: false },
      },
      targets: [...PAGES, PDF, DOCX, TXT],
      compress: "pdf",
      edit: null,
    };
  }
  if (/\.docx$/i.test(name)) {
    return {
      path,
      name,
      media: {
        container: "docx",
        kind: "text",
        durationSecs: null,
        sizeBytes: 3_400_000,
        bitRate: null,
        video: [],
        audio: [],
        coverArt: null,
        subtitleStreams: 0,
        tracks: [],
        hasTags: false,
        picture: null,
        hasLocation: false,
        document: null,
        text: { pictures: 4, widthPt: 595.3, heightPt: 841.9, simplified: params.get("mock") === "simplified" },
      },
      targets: [PDF, TXT],
      compress: "docx",
      edit: null,
    };
  }
  if (/\.svgz?$/i.test(name)) {
    // A drawing: rendered by Konvert, any size without loss.
    return {
      path,
      name,
      media: {
        container: "svg",
        kind: "image",
        durationSecs: null,
        sizeBytes: 4_200,
        bitRate: null,
        video: [{ index: 0, codec: "svg", width: 512, height: 512, fps: null, bitRate: null, pixFmt: "rgba" }],
        audio: [],
        coverArt: null,
        subtitleStreams: 0,
        tracks: [],
        hasTags: false,
        picture: { alpha: true, alphaStream: null, gridGroup: null, exif: false, exifCopyable: false },
        hasLocation: false,
        document: null,
      },
      targets: [...IMAGES, PDF],
      compress: null,
      edit: null,
    };
  }
  if (/\.(jpe?g|png|webp|avif|heic|heif|bmp|tiff?)$/i.test(name)) {
    const heic = /\.hei[cf]$/i.test(name);
    const ext = name.split(".").pop()!.toLowerCase();
    return {
      path,
      name,
      media: {
        container: heic ? "mov,mp4,m4a,3gp,3g2,mj2" : "image2",
        kind: "image",
        durationSecs: null,
        sizeBytes: 2_850_000,
        bitRate: null,
        video: [
          { index: 0, codec: heic ? "hevc" : "mjpeg", width: 3024, height: 4032, fps: null, bitRate: null, pixFmt: "yuvj420p" },
        ],
        audio: [],
        coverArt: null,
        subtitleStreams: 0,
        tracks: [],
        hasTags: false,
        picture: { alpha: false, alphaStream: null, gridGroup: heic ? 0 : null, exif: true, exifCopyable: true },
        hasLocation: true,
        document: null,
      },
      targets: [...IMAGES.map((o) => ({ ...o, sameAsInput: !heic && o.format === "jpg" })), PDF, TXT],
      // Like the core: pictures keep their format (no HEIC encoder, BMP and TIFF are not compressed).
      compress: /\.png$/i.test(name) ? "png" : /\.webp$/i.test(name) ? "webp" : /\.avif$/i.test(name) ? "avif" : /\.jpe?g$/i.test(name) ? "jpg" : null,
      // Like the core: an edited copy keeps the picture's format (none for HEIC).
      edit: heic ? null : (({ jpeg: "jpg", tif: "tiff" } as Record<string, TargetFormat>)[ext] ?? (ext as TargetFormat)),
    };
  }
  const isVideo = /\.(mp4|mov|mkv|webm|avi)$/i.test(name);
  const film = /film/i.test(name);
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
        audio: film
          ? [
              { index: 1, codec: "aac", sampleRate: 48000, channels: 2, bitRate: 192_000, bitDepth: null },
              { index: 2, codec: "ac3", sampleRate: 48000, channels: 6, bitRate: 448_000, bitDepth: null },
            ]
          : [{ index: 1, codec: "aac", sampleRate: 48000, channels: 2, bitRate: 192_000, bitDepth: null }],
        coverArt: null,
        subtitleStreams: film ? 2 : 0,
        // Films (mock files named "film"): two languages, text and picture subtitles.
        tracks: [
          { index: 0, kind: "video", codec: "h264", language: null, title: null, default: true, channels: null, bitmap: false },
          { index: 1, kind: "audio", codec: "aac", language: "ita", title: null, default: true, channels: 2, bitmap: false },
          ...(film
            ? ([
                { index: 2, kind: "audio", codec: "ac3", language: "eng", title: "Commento", default: false, channels: 6, bitmap: false },
                { index: 3, kind: "subtitle", codec: "subrip", language: "ita", title: null, default: false, channels: null, bitmap: false },
                { index: 4, kind: "subtitle", codec: "hdmv_pgs_subtitle", language: "eng", title: null, default: false, channels: null, bitmap: true },
              ] as const)
            : []),
        ],
        hasTags: false,
        hasLocation: false,
        document: null,
        picture: null,
      },
      targets: [
        ...VIDEO.map((o) => ({ ...o, sameAsInput: o.format === "mp4" })),
        ...AUDIO.map((o) => ({ ...o, group: "audioOnly" as const })),
        option("wav", "WAV", "audioOnly", true),
        ...FRAMES,
      ],
      compress: (name.split(".").pop()?.toLowerCase() ?? "mp4") as TargetFormat,
      edit: (name.split(".").pop()?.toLowerCase() ?? "mp4") as TargetFormat,
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
      tracks: [{ index: 0, kind: "audio", codec: "pcm_s16le", language: null, title: null, default: true, channels: 2, bitmap: false }],
      hasTags: true,
      hasLocation: false,
      document: null,
      picture: null,
    },
    targets: AUDIO,
    compress: "wav",
    edit: "wav",
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
    return { appVersion: "0.2.0-dev", ffmpegVersion: "ffmpeg version n9.0 (simulated)" };
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
    // "protetto" in the name: a PDF that opens with the password "segreta".
    if (/protett|protected/i.test(path) && !mockUnlocked.has(path)) {
      throw { code: "pdf_encrypted", details: null };
    }
    if (params.get("mock") === "error") {
      throw { code: "corrupt_input", details: "x.wav: Invalid data found when processing input" };
    }
    return analysis(path);
  },
  async pickPicture() {
    await sleep(200);
    return "C:\\Immagini\\Logo.png";
  },
  async unlockPdf(path, password) {
    await sleep(400);
    if (password !== "segreta") throw { code: "pdf_wrong_password", details: null };
    mockUnlocked.add(path);
    const found = analysis(path);
    return { ...found, media: { ...found.media, document: found.media.document && { ...found.media.document, locked: true } } };
  },
  async forgetPdfPassword(path) {
    mockUnlocked.delete(path);
  },
  async pdfThumbnails(_path, pages, size) {
    await sleep(250);
    return pages
      .filter((n) => n >= 1 && n <= MOCK_PAGES)
      .map((number) => {
        // Page 3 is landscape, like a table in a report.
        const [widthPt, heightPt] = number === 3 ? [841.9, 595.3] : [595.3, 841.9];
        return { number, widthPt, heightPt, dataUrl: mockPage(number, widthPt, heightPt, size) };
      });
  },
  async pictureThumbnail(path, size) {
    await sleep(300);
    const shown = analysis(path).media.video[0];
    return mockPhoto(path, shown?.width ?? 3, shown?.height ?? 4, size);
  },
  async pickAudioFile() {
    await sleep(150);
    return "C:\\Musica\\Colonna sonora.mp3";
  },
  async videoFrame(_path, atSecs, size) {
    await sleep(150);
    // A colored card with the time on it.
    const canvas = document.createElement("canvas");
    canvas.width = size;
    canvas.height = Math.round((size * 9) / 16);
    const ctx = canvas.getContext("2d")!;
    ctx.fillStyle = `hsl(${Math.round(atSecs * 3) % 360} 45% 45%)`;
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = "white";
    ctx.font = `${Math.round(size / 10)}px sans-serif`;
    ctx.fillText(`${atSecs.toFixed(1)} s`, size / 12, canvas.height / 2);
    return canvas.toDataURL("image/png");
  },
  async audioWaveform(_path, width, height) {
    await sleep(200);
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d")!;
    ctx.fillStyle = "#8a8a8e";
    for (let x = 0; x < width; x += 2) {
      const h = (Math.abs(Math.sin(x / 9) * Math.cos(x / 37)) * 0.8 + 0.1) * height;
      ctx.fillRect(x, (height - h) / 2, 1, h);
    }
    return canvas.toDataURL("image/png");
  },
  async planConversion(_path, media, request) {
    if (request.extract) {
      await sleep(80);
      // Like the core: labels from the language, else the kind and position.
      const used = new Set<string>();
      const extract = request.extract.tracks.map((chosen) => {
        const track = media.tracks.find((t) => t.index === chosen.index)!;
        const position = media.tracks.filter((t) => t.kind === track.kind).indexOf(track) + 1;
        let label = track.language ?? (track.kind === "video" ? "video" : `${track.kind === "audio" ? "audio" : "subtitles"}${position}`);
        for (let n = 2; used.has(label); n++) label = `${track.language}-${n}`;
        used.add(label);
        const extension =
          track.kind === "subtitle" ? (chosen.subtitles ?? "srt") : track.kind === "video" ? "mkv" : (chosen.audio ?? "m4a");
        return { index: track.index, kind: track.kind, extension, label, subtitleCodec: null };
      });
      const plan: ConversionPlan = {
        target: request.target,
        videoStream: null,
        video: null,
        audioStream: null,
        audio: null,
        coverArtStream: null,
        tracks: [],
        trim: null,
        extract,
        image: null,
        frames: null,
        document: null,
        removeLocation: false,
        warnings: [],
        estimate: null,
      };
      return plan;
    }
    if (request.compress) {
      await sleep(80);
      const level = { light: 0.75, strong: 0.45, max: 0.25 }[request.compress.level];
      const bytes = Math.round((media.sizeBytes ?? 0) * level);
      const capped = request.compress.maxBytes != null ? Math.min(bytes, request.compress.maxBytes) : bytes;
      const plan: ConversionPlan = {
        target: request.target,
        videoStream: null,
        video: null,
        audioStream: null,
        audio: null,
        coverArtStream: null,
        tracks: [],
        trim: null,
        extract: [],
        image: null,
        frames: null,
        document: null,
        removeLocation: request.removeLocation && media.hasLocation,
        compress: request.compress,
        warnings: media.kind === "document" && request.compress.pagesAsPictures ? ["pdfPagesAsPictures"] : [],
        // Like the core: a PDF's gain is only known once compressed.
        estimate: media.kind === "document" ? null : { bytes: capped, approximate: true },
      };
      return plan;
    }
    if (media.kind === "text" || request.target === "docx" || request.target === "txt") {
      await sleep(80);
      const plan: ConversionPlan = {
        target: request.target,
        videoStream: null,
        video: null,
        audioStream: null,
        audio: null,
        coverArtStream: null,
        tracks: [],
        trim: null,
        extract: [],
        image: null,
        frames: null,
        document: null,
        removeLocation: false,
        // Like the core: laid out (or read) by Konvert, size known once done.
        warnings:
          request.target === "txt"
            ? media.kind === "text"
              ? []
              : ["ocrReading"]
            : media.kind === "text"
              ? media.text?.simplified
                ? ["docxLayout", "docxSimplified"]
                : ["docxLayout"]
              : ["pdfToDocx"],
        estimate: null,
      };
      return plan;
    }
    if (request.target === "pdf" || media.kind === "document") {
      const toPdf = request.target === "pdf";
      const edited = toPdf && media.kind === "document";
      const count = media.document?.pages ?? 1;
      const layout = edited ? (request.pdf?.layout ?? Array.from({ length: count }, (_, i) => ({ number: i + 1, rotation: 0 }))) : [];
      if (edited && layout.length === 0) {
        throw { code: "invalid_pages", details: "no page left in the document" };
      }
      const pages = edited
        ? layout.map((p) => p.number)
        : toPdf
          ? Array.from({ length: count }, (_, i) => i + 1)
          : parsePages(request.pdf?.pages, count);
      const rotations = layout.some((p) => p.rotation) ? layout.map((p) => p.rotation) : [];
      const parts = edited ? splitSizes(request.pdf, pages.length) : [];
      const dpi = request.pdf?.dpi ?? 150;
      const plan: ConversionPlan = {
        target: request.target,
        videoStream: null,
        video: null,
        audioStream: null,
        audio: null,
        coverArtStream: null,
        tracks: [],
        trim: null,
        extract: [],
        image: null,
        frames: null,
        removeLocation: false,
        document: { pages, rotations, parts, dpi, placement: null },
        warnings: [
          ...(edited
            ? [
                ...(media.document?.bookmarkPages.some((p) => !pages.includes(p)) ? (["pdfBookmarksDropped"] as const) : []),
                ...(media.document?.form === "fixed" ? (["pdfFormFixed"] as const) : []),
                ...(media.document?.locked && !request.pdf?.protection ? (["pdfUnlocked"] as const) : []),
              ]
            : toPdf && media.picture?.exif
              ? (["metadataDropped"] as const)
              : !toPdf && pages.length > 300
                ? (["manyFiles"] as const)
                : []),
          ...(toPdf && request.pdf?.ocr ? (["ocrReading"] as const) : []),
        ],
        estimate: {
          bytes: toPdf
            ? Math.round(((media.sizeBytes ?? 0) * (edited ? pages.length / count : media.kind === "document" ? 1 : 0.6)))
            : Math.round(pages.length * 8.27 * 11.69 * dpi * dpi * (request.target === "png" ? 0.5 : 0.12)),
          approximate: true,
        },
      };
      await sleep(80);
      return plan;
    }
    const fromVideo = media.kind !== "image" && ["jpg", "png", "webp"].includes(request.target);
    if (fromVideo) {
      const frames = request.frames ?? { mode: "single", atSecs: null, everySecs: null };
      const duration = media.durationSecs ?? 0;
      const fps = media.video[0]?.fps ?? 25;
      const count =
        frames.mode === "single" ? 1 : Math.max(1, Math.ceil(frames.everySecs ? duration / frames.everySecs : duration * fps));
      const source = media.video[0];
      const plan: ConversionPlan = {
        target: request.target,
        videoStream: 0,
        video: null,
        audioStream: null,
        audio: null,
        coverArtStream: null,
        tracks: [],
        trim: null,
        extract: [],
        image: {
          codec: request.target === "jpg" ? "jpeg" : (request.target as "png" | "webp"),
          quality: request.target === "png" ? { kind: "lossless", effort: 6 } : { kind: "lossy", quality: 85 },
          width: source?.width ?? null,
          height: source?.height ?? null,
          resize: false,
          alpha: false,
          flatten: false,
          gridGroup: null,
          alphaStream: null,
          icons: [],
          turn: 0,
          flipHorizontal: false,
          flipVertical: false,
          crop: null,
          copyExif: false,
          render: null,
        },
        removeLocation: false,
        document: null,
        frames: {
          mode: frames.mode,
          atSecs: frames.mode === "single" ? (frames.atSecs ?? duration / 2) : null,
          everySecs: frames.mode === "single" ? null : (frames.everySecs ?? null),
          count,
        },
        warnings: count > 300 ? ["manyFiles"] : [],
        estimate: { bytes: count * 350_000, approximate: true },
      };
      return plan;
    }
    if (media.kind === "image") {
      const png = request.target === "png";
      const quality = { max: 95, balanced: 82, small: 65 }[request.preset];
      const source = media.video[0];
      const { stretched, ...size } = editedPicture(source?.width ?? 1000, source?.height ?? 1000, request.edit ?? null);
      const edited = { ...size, stretched };
      const plan: ConversionPlan = {
        target: request.target,
        videoStream: 0,
        video: null,
        audioStream: null,
        audio: null,
        coverArtStream: null,
        tracks: [],
        trim: null,
        extract: [],
        image: {
          codec: request.target === "jpg" ? "jpeg" : (request.target as "png" | "webp" | "avif"),
          quality: png ? { kind: "lossless", effort: 6 } : { kind: "lossy", quality: request.advanced.imageQuality ?? quality },
          ...edited,
          resize: false,
          alpha: false,
          flatten: false,
          gridGroup: media.picture?.gridGroup ?? null,
          alphaStream: null,
          icons: [],
          copyExif: ["jpg", "png", "webp", "avif"].includes(request.target),
          render: null,
          turn: request.edit?.rotation ?? 0,
          flipHorizontal: request.edit?.flipHorizontal ?? false,
          flipVertical: request.edit?.flipVertical ?? false,
          crop: null,
        },
        // EXIF data is copied into JPG, PNG, WebP and AVIF; other formats leave it out.
        warnings: [
          png ? "lossyToLossless" : "lossyReencode",
          ...(["jpg", "png", "webp", "avif"].includes(request.target) ? [] : (["metadataDropped"] as const)),
          ...(edited.stretched ? (["pictureStretched"] as const) : []),
        ],
        estimate: { bytes: Math.round((media.sizeBytes ?? 0) * (png ? 6 : quality / 100)), approximate: true },
        frames: null,
        removeLocation: request.removeLocation,
        document: null,
      };
      return plan;
    }
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
      tracks: [],
      trim: null,
      extract: [],
      image: null,
      frames: null,
      removeLocation: request.removeLocation,
      document: null,
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
  async startCombined(paths, request) {
    return mockBackend.startConversion(paths[0], { ...request, target: "pdf" });
  },
  async startConversion(path, request) {
    const jobId = nextJob++;
    const name = (path.split(/[\\/]/).pop() ?? "file").replace(/\.\w+$/, "");
    // Pages of a PDF: one picture next to it, or a folder of pictures.
    const pages = /\.pdf$/i.test(path) && request.target !== "pdf" ? parsePages(request.pdf?.pages, MOCK_PAGES) : null;
    const label = request.pdf?.pageLabel ?? "page";
    // A smaller copy: `?mock=nogain` simulates a file that cannot shrink.
    const compressed = request.compress
      ? params.get("mock") === "nogain"
        ? "nogain"
        : {
            output: `C:\\Documenti\\${name}-${request.compress.label ?? "compressed"}.${request.target}`,
            sizeBytes: Math.round(
              (analysis(path).media.sizeBytes ?? 0) * { light: 0.75, strong: 0.45, max: 0.25 }[request.compress.level],
            ),
            files: 1,
            elapsed: 1.2,
          }
      : null;
    // A split PDF: a folder of PDFs.
    const parts = /\.pdf$/i.test(path) && request.target === "pdf" ? splitSizes(request.pdf, request.pdf?.layout?.length ?? MOCK_PAGES) : [];
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
          if (compressed === "nogain") {
            emit({ jobId, update: { state: "failed", error: { code: "no_gain", details: null } } });
            return;
          }
          if (compressed) {
            emit({ jobId, update: { state: "done", outcome: compressed } });
            return;
          }
          // A series of frames is saved in a folder named after the video.
          const series = request.frames?.mode === "sequence" && ["jpg", "png", "webp"].includes(request.target);
          emit({
            jobId,
            update: {
              state: "done",
              outcome: series
                ? { output: `C:\\Video\\${name}`, sizeBytes: 42_000_000, files: 120, elapsed: 9.8 }
                : parts.length > 1
                  ? { output: `C:\\Documenti\\${name}`, sizeBytes: 1_300_000, files: parts.length, elapsed: 0.6 }
                  : pages && pages.length > 1
                  ? { output: `C:\\Documenti\\${name}`, sizeBytes: pages.length * 420_000, files: pages.length, elapsed: 2.4 }
                  : pages
                    ? {
                        output: `C:\\Documenti\\${name}-${label}-${String(pages[0]).padStart(2, "0")}.${request.target}`,
                        sizeBytes: 420_000,
                        files: 1,
                        elapsed: 0.4,
                      }
                    : {
                        // Like the core: an edited copy is `name-mod.ext`.
                        output: `C:\\Musica\\${name}${request.edit?.copy ? `-${request.edit.label ?? "mod"}` : ""}.${request.target}`,
                        sizeBytes: 5_400_000,
                        files: 1,
                        elapsed: 6.2,
                      },
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
