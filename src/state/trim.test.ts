import { describe, expect, it } from "vitest";

import { clampTrim, formatTime, isTrimmed, parseTime, trimLength } from "./trim";

describe("trim", () => {
  it("reads and writes times", () => {
    expect(formatTime(75.5)).toBe("00:01:15.500");
    expect(formatTime(3725.004)).toBe("01:02:05.004");
    expect(parseTime("00:01:15.500")).toBe(75.5);
    expect(parseTime("1:15,5")).toBe(75.5);
    expect(parseTime("90")).toBe(90);
    expect(parseTime("1:02:05")).toBe(3725);
    expect(parseTime("1:75")).toBeNull();
    expect(parseTime("1:2:3:4")).toBeNull();
    expect(parseTime("abc")).toBeNull();
  });

  it("keeps the part inside the file", () => {
    expect(clampTrim({ start: -3, end: 500, precise: null }, 120)).toEqual({ start: 0, end: null, precise: null });
    expect(clampTrim({ start: 50, end: 40, precise: null }, 120)).toMatchObject({ start: 39.9, end: 40 });
    expect(isTrimmed(null)).toBe(false);
    expect(isTrimmed({ start: 0, end: null, precise: true })).toBe(false);
    expect(isTrimmed({ start: 2, end: null, precise: null })).toBe(true);
    expect(trimLength({ start: 10, end: null, precise: null }, 120)).toBe(110);
  });
});
