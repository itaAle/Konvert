import { describe, expect, it } from "vitest";

import { chosenPages, pageLabels, placeBox } from "./stamps";

describe("watermark and page numbers", () => {
  it("chooses the pages of each document, like the core", () => {
    expect(chosenPages(null, 3)).toEqual([true, true, true]);
    expect(chosenPages("1, 3-", 4)).toEqual([true, false, true, true]);
    expect(chosenPages("2, 9", 3)).toEqual([false, true, false]);
    for (const wrong of ["0", "3-2", "a", "1-x"]) expect(chosenPages(wrong, 5), wrong).toBeNull();
  });

  it("numbers pages from the first one asked, skipped pages still counting", () => {
    const numbers = { position: "bottom" as const, format: "ofTotal" as const, word: null, size: 10, first: 2, start: 1, skip: "4" };
    expect(pageLabels(numbers, 5)).toEqual([null, "1 / 4", "2 / 4", null, "4 / 4"]);
    expect(pageLabels({ ...numbers, format: "word", word: "Pagina", first: 1, start: 10, skip: null }, 2)).toEqual(["Pagina 10", "Pagina 11"]);
  });

  it("places stamps like the core, smaller when too large", () => {
    const page = { width: 600, height: 800 };
    expect(placeBox("bottom", page, { width: 20, height: 10 }, 28, 0)).toEqual({ x: 300, y: 767, fit: 1 });
    expect(placeBox("topRight", page, { width: 20, height: 10 }, 28, 0)).toEqual({ x: 562, y: 33, fit: 1 });
    const long = placeBox("center", page, { width: 1200, height: 60 }, 0, 0);
    expect(long.x).toBe(300);
    expect(long.fit).toBeCloseTo(564 / 1200);
  });
});
