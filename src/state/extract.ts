import type { SubtitleFormat } from "../bindings/SubtitleFormat";
import type { TargetFormat } from "../bindings/TargetFormat";

/** Like the core: the audio format that keeps a codec as it is (FLAC otherwise). */
export function audioFormatFor(codec: string): TargetFormat {
  const formats: Record<string, TargetFormat> = {
    aac: "m4a",
    mp3: "mp3",
    opus: "opus",
    vorbis: "ogg",
    flac: "flac",
    alac: "alac",
    ac3: "ac3",
    eac3: "eac3",
    mp2: "mp2",
    wmav2: "wma",
  };
  return formats[codec] ?? (codec.startsWith("pcm_") ? "wav" : "flac");
}

/** Like the core: the subtitle format closest to a codec. */
export function subtitleFormatFor(codec: string): SubtitleFormat {
  if (codec === "ass" || codec === "ssa") return "ass";
  if (codec === "webvtt") return "vtt";
  return "srt";
}
