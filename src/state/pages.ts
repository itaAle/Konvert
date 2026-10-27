// Page editor of a single PDF: the order of its pages, their turns, the pages
// left out and where a split starts a new document. Pure helpers; the queue
// keeps the state and turns it into the request sent to the core.
import type { PdfPage } from "../bindings/PdfPage";

export interface EditorPage {
  /** 1-based page number in the original document. */
  number: number;
  /** Clockwise turn added to the page, in degrees: 0, 90, 180 or 270. */
  rotation: number;
  /** Left out of the new document (still shown, so it can be put back). */
  removed: boolean;
}

/** Every page of a document, as it is. */
export function initialPages(count: number): EditorPage[] {
  return Array.from({ length: count }, (_, i) => ({ number: i + 1, rotation: 0, removed: false }));
}

/** Some page was turned, left out or moved. */
export function isEdited(pages: EditorPage[] | null): boolean {
  return pages != null && pages.some((p, i) => p.removed || p.rotation !== 0 || p.number !== i + 1);
}

const turn = (rotation: number, delta: number) => (((rotation + delta) % 360) + 360) % 360;

export function rotatePage(pages: EditorPage[], index: number, delta: 90 | -90): EditorPage[] {
  return pages.map((p, i) => (i === index ? { ...p, rotation: turn(p.rotation, delta) } : p));
}

/** Turns every page that is kept. */
export function rotateAll(pages: EditorPage[], delta: 90 | -90): EditorPage[] {
  return pages.map((p) => (p.removed ? p : { ...p, rotation: turn(p.rotation, delta) }));
}

/** Leaves a page out or puts it back; the last page left cannot go. */
export function toggleRemoved(pages: EditorPage[], index: number): EditorPage[] {
  const page = pages[index];
  if (!page || (!page.removed && keptCount(pages) <= 1)) return pages;
  return pages.map((p, i) => (i === index ? { ...p, removed: !p.removed } : p));
}

/** Moves the page at `from` so that it ends up at index `to`. */
export function movePage(pages: EditorPage[], from: number, to: number): EditorPage[] {
  if (from === to || from < 0 || from >= pages.length || to < 0 || to >= pages.length) return pages;
  const next = [...pages];
  const [page] = next.splice(from, 1);
  next.splice(to, 0, page);
  return next;
}

/** Keeps only the listed page numbers (the others are left out, the listed ones put back). */
export function keepOnly(pages: EditorPage[], numbers: number[]): EditorPage[] {
  const keep = new Set(numbers);
  if (!pages.some((p) => keep.has(p.number))) return pages;
  return pages.map((p) => ({ ...p, removed: !keep.has(p.number) }));
}

export function keptCount(pages: EditorPage[]): number {
  return pages.filter((p) => !p.removed).length;
}

/** Pages of the new document for the core (`null`: every page as it is). */
export function layoutOf(pages: EditorPage[] | null): PdfPage[] | null {
  if (!pages || !isEdited(pages)) return null;
  return pages.filter((p) => !p.removed).map(({ number, rotation }) => ({ number, rotation }));
}

/**
 * Positions in the new document after which a new one starts. Cuts are kept
 * as the number of the page they follow, so they move with it; a cut after a
 * page left out, or after the last page, does nothing.
 */
export function cutPositions(pages: EditorPage[] | null, cuts: number[], count: number): number[] {
  const kept = (pages ?? initialPages(count)).filter((p) => !p.removed);
  const after = new Set(cuts);
  return kept.flatMap((p, i) => (after.has(p.number) && i < kept.length - 1 ? [i + 1] : []));
}

export function toggleCut(cuts: number[], number: number): number[] {
  return cuts.includes(number) ? cuts.filter((c) => c !== number) : [...cuts, number].sort((a, b) => a - b);
}

/** Positions after which a new document starts when splitting every `every` pages. */
export function everyPositions(kept: number, every: number): number[] {
  if (every < 1) return [];
  return Array.from({ length: Math.ceil(kept / every) - 1 }, (_, i) => (i + 1) * every);
}

/**
 * Page numbers written by the user, in the same syntax the core accepts for
 * pictures of pages ("1-3, 5, 8-"); `null` when the text is not valid.
 */
export function parseRange(text: string, count: number): number[] | null {
  const parts = text
    .split(/[,;]/)
    .map((p) => p.trim())
    .filter(Boolean);
  if (parts.length === 0) return null;
  const chosen = new Set<number>();
  for (const part of parts) {
    const m = part.match(/^(\d*)\s*(-?)\s*(\d*)$/);
    if (!m || (!m[1] && !m[3])) return null;
    const first = m[1] ? Number(m[1]) : 1;
    const last = m[3] ? Number(m[3]) : m[2] ? count : first;
    if (!(first >= 1 && first <= last && last <= count)) return null;
    for (let p = first; p <= last; p++) chosen.add(p);
  }
  return [...chosen].sort((a, b) => a - b);
}
