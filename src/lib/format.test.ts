import { describe, expect, it } from "vitest";
import {
  codecName,
  describeMedia,
  formatBytes,
  formatDuration,
  formatRemaining,
  formatSampleRate,
} from "./format";
import type { MediaInfo } from "../bindings/MediaInfo";

const media = (overrides: Partial<MediaInfo>): MediaInfo => ({
  container: "wav",
  kind: "audio",
  durationSecs: 10,
  sizeBytes: 1000,
  bitRate: null,
  audio: [],
  video: [],
  coverArt: null,
  subtitleStreams: 0,
  ...overrides,
});

describe("formatBytes", () => {
  it("uses Italian decimal separators and decimal units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1,5 KB");
    expect(formatBytes(4_200_000)).toBe("4,2 MB");
    expect(formatBytes(734_000_000)).toBe("734 MB");
    expect(formatBytes(2_500_000_000)).toBe("2,5 GB");
  });
});

describe("formatDuration", () => {
  it("formats minutes and hours", () => {
    expect(formatDuration(5)).toBe("0:05");
    expect(formatDuration(75)).toBe("1:15");
    expect(formatDuration(3725)).toBe("1:02:05");
  });
});

describe("formatRemaining", () => {
  it("rounds to friendly values", () => {
    expect(formatRemaining(3)).toBe("pochi secondi");
    expect(formatRemaining(42)).toBe("40 s");
    expect(formatRemaining(185)).toBe("3 min");
    expect(formatRemaining(3900)).toBe("1 h 5 min");
  });
});

describe("media details", () => {
  it("names codecs", () => {
    expect(codecName("pcm_s24le")).toBe("PCM");
    expect(codecName("h264")).toBe("H.264");
    expect(codecName("weird")).toBe("WEIRD");
    expect(formatSampleRate(44100)).toBe("44,1 kHz");
  });

  it("describes audio files", () => {
    const wav = media({
      audio: [{ index: 0, codec: "pcm_s16le", sampleRate: 44100, channels: 2, bitRate: null, bitDepth: 16 }],
    });
    expect(describeMedia(wav)).toBe("PCM · 44,1 kHz · stereo · 16 bit");
  });

  it("describes videos with and without audio", () => {
    const video = { index: 0, codec: "h264", width: 1920, height: 1080, fps: 29.97, bitRate: null, pixFmt: null };
    const withAudio = media({
      kind: "videoWithAudio",
      video: [video],
      audio: [{ index: 1, codec: "aac", sampleRate: 48000, channels: 6, bitRate: null, bitDepth: null }],
    });
    expect(describeMedia(withAudio)).toBe("1920×1080 · 29,97 fps · H.264 · AAC 6 canali");
    expect(describeMedia(media({ kind: "video", video: [video] }))).toBe(
      "1920×1080 · 29,97 fps · H.264 · senza audio",
    );
  });
});
