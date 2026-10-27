import { describe, expect, it } from "vitest";

import {
  cutPositions,
  everyPositions,
  initialPages,
  isEdited,
  keepOnly,
  keptCount,
  layoutOf,
  movePage,
  parseRange,
  rotateAll,
  rotatePage,
  toggleCut,
  toggleRemoved,
} from "./pages";

const numbers = (pages: { number: number }[]) => pages.map((p) => p.number);

describe("page editor", () => {
  it("starts with every page as it is", () => {
    const pages = initialPages(3);
    expect(numbers(pages)).toEqual([1, 2, 3]);
    expect(isEdited(pages)).toBe(false);
    expect(isEdited(null)).toBe(false);
    expect(layoutOf(pages)).toBeNull();
  });

  it("turns pages both ways, alone or all together", () => {
    let pages = rotatePage(initialPages(2), 0, -90);
    expect(pages[0].rotation).toBe(270);
    pages = rotatePage(pages, 0, 90);
    expect(pages[0].rotation).toBe(0);
    expect(isEdited(pages)).toBe(false);
    pages = rotateAll(toggleRemoved(initialPages(2), 1), 90);
    // Pages left out are not turned.
    expect(pages.map((p) => p.rotation)).toEqual([90, 0]);
    expect(layoutOf(pages)).toEqual([{ number: 1, rotation: 90 }]);
  });

  it("leaves pages out and puts them back, never the last one", () => {
    let pages = toggleRemoved(initialPages(2), 0);
    expect(keptCount(pages)).toBe(1);
    expect(toggleRemoved(pages, 1)).toBe(pages);
    pages = toggleRemoved(pages, 0);
    expect(isEdited(pages)).toBe(false);
  });

  it("moves pages and sends the new order", () => {
    const pages = movePage(initialPages(4), 0, 3);
    expect(numbers(pages)).toEqual([2, 3, 4, 1]);
    expect(numbers(movePage(pages, 3, 0))).toEqual([1, 2, 3, 4]);
    expect(movePage(pages, 1, 9)).toBe(pages);
    expect(layoutOf(pages)?.map((p) => p.number)).toEqual([2, 3, 4, 1]);
  });

  it("keeps only the pages written by the user", () => {
    const range = parseRange("4-, 1", 5)!;
    expect(range).toEqual([1, 4, 5]);
    const pages = keepOnly(initialPages(5), range);
    expect(layoutOf(pages)?.map((p) => p.number)).toEqual([1, 4, 5]);
    // Nothing would be left: unchanged.
    const none = initialPages(2);
    expect(keepOnly(none, [7])).toBe(none);
    for (const wrong of ["", "0", "6", "3-2", "a", "-", "1--2"]) expect(parseRange(wrong, 5), wrong).toBeNull();
    expect(parseRange("-2", 5)).toEqual([1, 2]);
  });

  it("places cuts after pages, following them when they move", () => {
    const cuts = toggleCut(toggleCut([], 3), 1);
    expect(cuts).toEqual([1, 3]);
    expect(toggleCut(cuts, 1)).toEqual([3]);
    // Untouched document: positions are page numbers.
    expect(cutPositions(null, cuts, 5)).toEqual([1, 3]);
    // Page 3 moves to the front: its cut follows it; a cut after the last page does nothing.
    const moved = movePage(initialPages(5), 2, 0);
    expect(cutPositions(moved, cuts, 5)).toEqual([1, 2]);
    expect(cutPositions(initialPages(3), [3], 3)).toEqual([]);
    // A cut after a page left out does nothing.
    expect(cutPositions(toggleRemoved(initialPages(5), 0), cuts, 5)).toEqual([2]);
  });

  it("shows where splitting every few pages cuts", () => {
    expect(everyPositions(5, 2)).toEqual([2, 4]);
    expect(everyPositions(4, 2)).toEqual([2]);
    expect(everyPositions(3, 1)).toEqual([1, 2]);
    expect(everyPositions(3, 5)).toEqual([]);
  });
});
