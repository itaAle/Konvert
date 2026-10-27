import { describe, expect, it } from "vitest";

import {
  cutPositions,
  everyPositions,
  initialPages,
  isEdited,
  keepOnly,
  keptCount,
  layoutOf,
  moveFile,
  movePage,
  pageId,
  parseRange,
  reconcile,
  rotateAll,
  rotatePage,
  toggleCut,
  toggleRemoved,
  type PageSource,
} from "./pages";

const numbers = (pages: { number: number }[]) => pages.map((p) => p.number);
/** One PDF of `count` pages (key 1). */
const one = (count: number): PageSource[] => [{ key: 1, count }];
const ids = (pages: { file: number; number: number }[]) => pages.map(pageId);

describe("page editor", () => {
  it("starts with every page as it is", () => {
    const pages = initialPages(one(3));
    expect(numbers(pages)).toEqual([1, 2, 3]);
    expect(isEdited(pages, one(3))).toBe(false);
    expect(isEdited(null, one(3))).toBe(false);
    expect(layoutOf(pages, one(3))).toBeNull();
  });

  it("turns pages both ways, alone or all together", () => {
    let pages = rotatePage(initialPages(one(2)), 0, -90);
    expect(pages[0].rotation).toBe(270);
    pages = rotatePage(pages, 0, 90);
    expect(pages[0].rotation).toBe(0);
    expect(isEdited(pages, one(2))).toBe(false);
    pages = rotateAll(toggleRemoved(initialPages(one(2)), 1), 90);
    // Pages left out are not turned.
    expect(pages.map((p) => p.rotation)).toEqual([90, 0]);
    expect(layoutOf(pages, one(2))).toEqual([{ file: 0, number: 1, rotation: 90 }]);
  });

  it("leaves pages out and puts them back, never the last one", () => {
    let pages = toggleRemoved(initialPages(one(2)), 0);
    expect(keptCount(pages)).toBe(1);
    expect(toggleRemoved(pages, 1)).toBe(pages);
    pages = toggleRemoved(pages, 0);
    expect(isEdited(pages, one(2))).toBe(false);
  });

  it("moves pages and sends the new order", () => {
    const pages = movePage(initialPages(one(4)), 0, 3);
    expect(numbers(pages)).toEqual([2, 3, 4, 1]);
    expect(numbers(movePage(pages, 3, 0))).toEqual([1, 2, 3, 4]);
    expect(movePage(pages, 1, 9)).toBe(pages);
    expect(layoutOf(pages, one(4))?.map((p) => p.number)).toEqual([2, 3, 4, 1]);
  });

  it("keeps only the pages written by the user", () => {
    const range = parseRange("4-, 1", 5)!;
    expect(range).toEqual([1, 4, 5]);
    const pages = keepOnly(initialPages(one(5)), range);
    expect(layoutOf(pages, one(5))?.map((p) => p.number)).toEqual([1, 4, 5]);
    // Nothing would be left: unchanged.
    const none = initialPages(one(2));
    expect(keepOnly(none, [7])).toBe(none);
    for (const wrong of ["", "0", "6", "3-2", "a", "-", "1--2"]) expect(parseRange(wrong, 5), wrong).toBeNull();
    expect(parseRange("-2", 5)).toEqual([1, 2]);
  });

  it("places cuts after pages, following them when they move", () => {
    const cuts = toggleCut(toggleCut([], "1:3"), "1:1");
    expect(cuts).toEqual(["1:3", "1:1"]);
    expect(toggleCut(cuts, "1:1")).toEqual(["1:3"]);
    // Untouched document: positions are page numbers.
    expect(cutPositions(null, cuts, one(5))).toEqual([1, 3]);
    // Page 3 moves to the front: its cut follows it; a cut after the last page does nothing.
    const moved = movePage(initialPages(one(5)), 2, 0);
    expect(cutPositions(moved, cuts, one(5))).toEqual([1, 2]);
    expect(cutPositions(initialPages(one(3)), ["1:3"], one(3))).toEqual([]);
    // A cut after a page left out does nothing.
    expect(cutPositions(toggleRemoved(initialPages(one(5)), 0), cuts, one(5))).toEqual([2]);
  });

  it("shows where splitting every few pages cuts", () => {
    expect(everyPositions(5, 2)).toEqual([2, 4]);
    expect(everyPositions(4, 2)).toEqual([2]);
    expect(everyPositions(3, 1)).toEqual([1, 2]);
    expect(everyPositions(3, 5)).toEqual([]);
  });
});

describe("page editor with several files", () => {
  // A PDF of two pages (key 4), a photo (key 7), a PDF of one page (key 9).
  const sources: PageSource[] = [
    { key: 4, count: 2 },
    { key: 7, count: 1 },
    { key: 9, count: 1 },
  ];

  it("lists the pages of every file in order", () => {
    const pages = initialPages(sources);
    expect(ids(pages)).toEqual(["4:1", "4:2", "7:1", "9:1"]);
    expect(isEdited(pages, sources)).toBe(false);
    expect(layoutOf(pages, sources)).toBeNull();
  });

  it("sends pages moved between files with the position of their file", () => {
    let pages = movePage(initialPages(sources), 3, 0);
    pages = rotatePage(toggleRemoved(pages, 2), 1, 90);
    expect(ids(pages)).toEqual(["9:1", "4:1", "4:2", "7:1"]);
    expect(layoutOf(pages, sources)).toEqual([
      { file: 2, number: 1, rotation: 0 },
      { file: 0, number: 1, rotation: 90 },
      { file: 1, number: 1, rotation: 0 },
    ]);
    // Cuts name the pages of each file.
    expect(cutPositions(pages, ["4:1", "7:1"], sources)).toEqual([2]);
  });

  it("follows the files of the list", () => {
    const edited = movePage(initialPages(sources), 2, 0);
    // A file leaves: its pages go; a new one: its pages come at the end.
    const next = reconcile(edited, [sources[0], sources[1], { key: 11, count: 2 }]);
    expect(ids(next)).toEqual(["7:1", "4:1", "4:2", "11:1", "11:2"]);
    expect(ids(reconcile(null, sources))).toEqual(ids(initialPages(sources)));
  });

  it("moves every page of a file past the file next to it", () => {
    // The photo was put in the middle of the first PDF.
    const pages = movePage(initialPages(sources), 2, 1);
    const order = sources.map((s) => s.key);
    expect(ids(pages)).toEqual(["4:1", "7:1", "4:2", "9:1"]);
    // The first PDF goes after the photo.
    expect(ids(moveFile(pages, order, 4, 1))).toEqual(["7:1", "4:1", "4:2", "9:1"]);
    // The last PDF goes before the photo.
    expect(ids(moveFile(pages, order, 9, -1))).toEqual(["4:1", "9:1", "7:1", "4:2"]);
    // Nothing above the first file.
    expect(moveFile(pages, order, 4, -1)).toBe(pages);
  });
});
