import type { Track } from "../bindings/Track";
import type { TrackChoice } from "../bindings/TrackChoice";
import type { TrackKind } from "../bindings/TrackKind";

/** A track as the file describes it. */
export function choiceOf(track: Track): TrackChoice {
  return { index: track.index, language: track.language, title: track.title, default: track.default };
}

/**
 * The tracks kept when the user changes nothing: every track for an edited
 * copy; otherwise, like the core, the first video and the first audio track.
 */
export function defaultTracks(tracks: Track[], copy: boolean): TrackChoice[] {
  if (copy) return tracks.map(choiceOf);
  const video = tracks.find((t) => t.kind === "video");
  const audio = tracks.find((t) => t.kind === "audio");
  return [video, audio].filter((t): t is Track => t != null).map(choiceOf);
}

/** Kept tracks in output order: video, audio, subtitles (each kind in the chosen order). */
export function ordered(choices: TrackChoice[], tracks: Track[]): TrackChoice[] {
  const kind = (c: TrackChoice) => tracks.find((t) => t.index === c.index)?.kind ?? "subtitle";
  const rank: Record<TrackKind, number> = { video: 0, audio: 1, subtitle: 2 };
  return choices.map((c, i) => ({ c, i })).sort((a, b) => rank[kind(a.c)] - rank[kind(b.c)] || a.i - b.i).map((x) => x.c);
}

/** The track kept or left out (kept at the end of its kind). */
export function toggleTrack(choices: TrackChoice[], tracks: Track[], index: number): TrackChoice[] {
  if (choices.some((c) => c.index === index)) return choices.filter((c) => c.index !== index);
  const track = tracks.find((t) => t.index === index);
  return track ? ordered([...choices, { ...choiceOf(track), default: false }], tracks) : choices;
}

/** The track moved one place up or down among the kept tracks of its kind. */
export function moveTrack(choices: TrackChoice[], tracks: Track[], index: number, delta: -1 | 1): TrackChoice[] {
  const kind = tracks.find((t) => t.index === index)?.kind;
  const same = choices.filter((c) => tracks.find((t) => t.index === c.index)?.kind === kind);
  const from = same.findIndex((c) => c.index === index);
  const to = from + delta;
  if (from < 0 || to < 0 || to >= same.length) return choices;
  [same[from], same[to]] = [same[to], same[from]];
  const others = choices.filter((c) => !same.includes(c));
  return ordered([...others, ...same], tracks);
}

/** The track becomes the default of its kind (or stops being it). */
export function setDefaultTrack(choices: TrackChoice[], tracks: Track[], index: number): TrackChoice[] {
  const kind = tracks.find((t) => t.index === index)?.kind;
  const on = !choices.find((c) => c.index === index)?.default;
  return choices.map((c) => {
    if (c.index === index) return { ...c, default: on };
    const sameKind = tracks.find((t) => t.index === c.index)?.kind === kind;
    return sameKind && on ? { ...c, default: false } : c;
  });
}

/** Same tracks, order and details. */
export function sameTracks(a: TrackChoice[], b: TrackChoice[]): boolean {
  return (
    a.length === b.length &&
    a.every(
      (c, i) =>
        c.index === b[i].index &&
        (c.language ?? null) === (b[i].language ?? null) &&
        (c.title ?? "") === (b[i].title ?? "") &&
        c.default === b[i].default,
    )
  );
}

/** Common languages, as ISO 639-2 codes (the ones files use) with their ISO 639-1 code for names. */
export const LANGUAGES: [string, string][] = [
  ["ita", "it"],
  ["eng", "en"],
  ["fra", "fr"],
  ["deu", "de"],
  ["spa", "es"],
  ["por", "pt"],
  ["nld", "nl"],
  ["pol", "pl"],
  ["ron", "ro"],
  ["rus", "ru"],
  ["ukr", "uk"],
  ["ell", "el"],
  ["tur", "tr"],
  ["ara", "ar"],
  ["heb", "he"],
  ["hin", "hi"],
  ["zho", "zh"],
  ["jpn", "ja"],
  ["kor", "ko"],
  ["swe", "sv"],
  ["nor", "no"],
  ["dan", "da"],
  ["fin", "fi"],
  ["ces", "cs"],
  ["hun", "hu"],
];

/** Other spellings files use (ISO 639-2/B and 639-1). */
const ALIASES: Record<string, string> = { fre: "fra", ger: "deu", dut: "nld", rum: "ron", gre: "ell", chi: "zho", cze: "ces" };

/** The language code as listed (`ger` → `deu`, `it` → `ita`). */
export function normalLanguage(code: string | null): string | null {
  if (!code) return null;
  const lower = code.toLowerCase();
  return ALIASES[lower] ?? LANGUAGES.find(([, two]) => two === lower)?.[0] ?? lower;
}

/** A language's name in the interface language (the code when unknown). */
export function languageName(code: string, locale: string): string {
  const two = LANGUAGES.find(([three]) => three === code)?.[1] ?? code;
  try {
    const name = new Intl.DisplayNames([locale], { type: "language" }).of(two);
    return name && name !== two ? name.charAt(0).toUpperCase() + name.slice(1) : code;
  } catch {
    return code;
  }
}
