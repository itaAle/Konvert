// Display names of the encoders offered in the advanced options.
import type { AudioCodec } from "../bindings/AudioCodec";
import type { VideoCodec } from "../bindings/VideoCodec";

export const VIDEO_CODEC_NAMES: Record<VideoCodec, string> = {
  h264: "H.264",
  av1: "AV1",
  vp9: "VP9",
  mpeg4: "MPEG-4",
  gif: "GIF",
};

export const AUDIO_CODEC_NAMES: Record<AudioCodec, string> = {
  mp3: "MP3",
  aac: "AAC",
  pcm: "PCM",
  flac: "FLAC",
  vorbis: "Vorbis",
  opus: "Opus",
  wma: "WMA",
};

/** Highest sensible bit rate per lossy encoder (kbit/s, stereo). */
export const MAX_AUDIO_BITRATE: Partial<Record<AudioCodec, number>> = {
  mp3: 320,
  wma: 192,
  aac: 512,
  opus: 510,
  vorbis: 500,
};
