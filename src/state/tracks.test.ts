import { describe, expect, it } from "vitest";

import type { Track } from "../bindings/Track";
import { defaultTracks, languageName, moveTrack, normalLanguage, sameTracks, setDefaultTrack, toggleTrack } from "./tracks";

const track = (index: number, kind: Track["kind"], language: string | null, isDefault = false): Track => ({
  index,
  kind,
  codec: "x",
  language,
  title: null,
  default: isDefault,
  channels: null,
  bitmap: false,
});
const tracks = [track(0, "video", null, true), track(1, "audio", "ita", true), track(2, "audio", "eng"), track(3, "subtitle", "ita")];

describe("tracks", () => {
  it("keeps the core's choice, or everything in a copy", () => {
    expect(defaultTracks(tracks, false).map((c) => c.index)).toEqual([0, 1]);
    expect(defaultTracks(tracks, true).map((c) => c.index)).toEqual([0, 1, 2, 3]);
  });

  it("keeps, moves and marks tracks within their kind", () => {
    let choice = defaultTracks(tracks, false);
    // A track kept again goes at the end of its kind, before later kinds.
    choice = toggleTrack(choice, tracks, 3);
    choice = toggleTrack(choice, tracks, 2);
    expect(choice.map((c) => c.index)).toEqual([0, 1, 2, 3]);
    choice = moveTrack(choice, tracks, 2, -1);
    expect(choice.map((c) => c.index)).toEqual([0, 2, 1, 3]);
    // The first audio cannot go above the video.
    expect(moveTrack(choice, tracks, 2, -1)).toEqual(choice);
    // One default per kind.
    choice = setDefaultTrack(choice, tracks, 2);
    expect(choice.filter((c) => c.default).map((c) => c.index)).toEqual([0, 2]);
    choice = setDefaultTrack(choice, tracks, 2);
    expect(choice.filter((c) => c.default).map((c) => c.index)).toEqual([0]);
    expect(toggleTrack(choice, tracks, 1).map((c) => c.index)).toEqual([0, 2, 3]);
    expect(sameTracks(choice, [...choice])).toBe(true);
    expect(sameTracks(choice, defaultTracks(tracks, true))).toBe(false);
  });

  it("names languages", () => {
    expect(normalLanguage("ger")).toBe("deu");
    expect(normalLanguage("it")).toBe("ita");
    expect(normalLanguage(null)).toBeNull();
    expect(languageName("ita", "it")).toBe("Italiano");
    expect(languageName("eng", "en")).toBe("English");
    expect(languageName("qaa", "it")).toBe("qaa");
  });
});
