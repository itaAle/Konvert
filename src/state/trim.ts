/** The part of an audio or video file that is kept. */
export interface TrimState {
  /** Seconds from the start. */
  start: number;
  /** Seconds from the start (`null`: the end of the file). */
  end: number | null;
  /**
   * Cut at the exact moment, encoding again (`null`: the default, fast for
   * an edited copy and precise for a conversion, which encodes anyway).
   */
  precise: boolean | null;
}

/** Shortest part kept, in seconds. */
export const MIN_LENGTH = 0.1;

/** `75.5` → `"00:01:15.500"`. */
export function formatTime(seconds: number): string {
  const ms = Math.max(0, Math.round(seconds * 1000));
  const h = Math.floor(ms / 3_600_000);
  const m = Math.floor((ms % 3_600_000) / 60_000);
  const s = Math.floor((ms % 60_000) / 1000);
  const rest = ms % 1000;
  const two = (v: number) => String(v).padStart(2, "0");
  return `${two(h)}:${two(m)}:${two(s)}.${String(rest).padStart(3, "0")}`;
}

/**
 * Seconds of a time typed as `hh:mm:ss.mmm`, `mm:ss`, `ss` or `ss.mmm`
 * (commas allowed for the decimals); `null` when it is not a time.
 */
export function parseTime(text: string): number | null {
  const clean = text.trim().replace(",", ".");
  if (!/^\d+(:\d{1,2}){0,2}(\.\d+)?$/.test(clean)) return null;
  const parts = clean.split(":");
  const seconds = Number(parts.pop());
  const minutes = parts.length ? Number(parts.pop()) : 0;
  const hours = parts.length ? Number(parts.pop()) : 0;
  if ((clean.includes(":") && seconds >= 60) || (hours > 0 && minutes >= 60)) return null;
  return hours * 3600 + minutes * 60 + seconds;
}

/** The trim kept inside the file, at least `MIN_LENGTH` long. */
export function clampTrim(trim: TrimState, duration: number): TrimState {
  const end = trim.end == null ? duration : Math.min(Math.max(trim.end, MIN_LENGTH), duration);
  const start = Math.min(Math.max(trim.start, 0), Math.max(0, end - MIN_LENGTH));
  return { ...trim, start, end: end >= duration - 0.0005 ? null : end };
}

/** Something is cut away. */
export function isTrimmed(trim: TrimState | null): boolean {
  return trim != null && (trim.start > 0.0005 || trim.end != null);
}

/** Length of the part kept. */
export function trimLength(trim: TrimState | null, duration: number): number {
  if (!trim) return duration;
  return (trim.end ?? duration) - trim.start;
}
