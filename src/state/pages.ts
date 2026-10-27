// Page editor of the PDF being made: the order of the pages of one PDF, or
// of several files combined (PDFs and photos), their turns, the pages left
// out and where a split starts a new document. Pure helpers; the queue keeps
// the state and turns it into the request sent to the core.
import type { PdfPage } from "../bindings/PdfPage";

/** A file whose pages are in the editor. */
export interface PageSource {
  /** Key of the file in the queue (stable while it is in the list). */
  key: number;
  /** Its pages (a photo is one page). */
  count: number;
}

export interface EditorPage {
  /** Key of the file the page comes from. */
  file: number;
  /** 1-based page number in that file. */
  number: number;
  /** Clockwise turn added to the page, in degrees: 0, 90, 180 or 270. */
  rotation: number;
  /** Left out of the new document (still shown, so it can be put back). */
  removed: boolean;
}

/** Identifies a page among the pages of every file. */
export function pageId(page: Pick<EditorPage, "file" | "number">): string {
  return `${page.file}:${page.number}`;
}

/** Every page of the files, in order, as they are. */
export function initialPages(sources: PageSource[]): EditorPage[] {
  return sources.flatMap((s) => Array.from({ length: s.count }, (_, i) => ({ file: s.key, number: i + 1, rotation: 0, removed: false })));
}

/** Some page was turned, left out or moved. */
export function isEdited(pages: EditorPage[] | null, sources: PageSource[]): boolean {
  if (pages == null) return false;
  const initial = initialPages(sources);
  return (
    pages.length !== initial.length ||
    pages.some((p, i) => p.removed || p.rotation !== 0 || p.file !== initial[i].file || p.number !== initial[i].number)
  );
}

/**
 * The edited pages for the files now in the list: pages of files that left
 * are dropped, every page of a new file is added at the end.
 */
export function reconcile(pages: EditorPage[] | null, sources: PageSource[]): EditorPage[] {
  if (pages == null) return initialPages(sources);
  const counts = new Map(sources.map((s) => [s.key, s.count]));
  const kept = pages.filter((p) => p.number <= (counts.get(p.file) ?? 0));
  const present = new Set(kept.map((p) => p.file));
  return [...kept, ...initialPages(sources.filter((s) => !present.has(s.key)))];
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

/**
 * Moves every page of a file, together, past the pages of the file next to
 * it in the list (`delta` -1: before the first page of the file above; 1:
 * after the last page of the file below). `order` is the list before the move.
 */
export function moveFile(pages: EditorPage[], order: number[], key: number, delta: -1 | 1): EditorPage[] {
  const neighbour = order[order.indexOf(key) + delta];
  if (neighbour == null || !order.includes(key)) return pages;
  const moving = pages.filter((p) => p.file === key);
  const rest = pages.filter((p) => p.file !== key);
  const at =
    delta < 0
      ? rest.findIndex((p) => p.file === neighbour)
      : rest.length - [...rest].reverse().findIndex((p) => p.file === neighbour);
  if (at < 0 || at > rest.length) return pages;
  return [...rest.slice(0, at), ...moving, ...rest.slice(at)];
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
export function layoutOf(pages: EditorPage[] | null, sources: PageSource[]): PdfPage[] | null {
  if (!pages || !isEdited(pages, sources)) return null;
  const index = new Map(sources.map((s, i) => [s.key, i]));
  return pages
    .filter((p) => !p.removed && index.has(p.file))
    .map(({ file, number, rotation }) => ({ file: index.get(file)!, number, rotation }));
}

/**
 * Positions in the new document after which a new one starts. Cuts are kept
 * as the page they follow (its `pageId`), so they move with it; a cut after a
 * page left out, or after the last page, does nothing.
 */
export function cutPositions(pages: EditorPage[] | null, cuts: string[], sources: PageSource[]): number[] {
  const kept = (pages ?? initialPages(sources)).filter((p) => !p.removed);
  const after = new Set(cuts);
  return kept.flatMap((p, i) => (after.has(pageId(p)) && i < kept.length - 1 ? [i + 1] : []));
}

export function toggleCut(cuts: string[], id: string): string[] {
  return cuts.includes(id) ? cuts.filter((c) => c !== id) : [...cuts, id];
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
