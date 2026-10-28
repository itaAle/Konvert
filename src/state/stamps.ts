// Watermark and page numbers of the PDF being made, by the same rules as the
// core (`pdf_stamp.rs`): used to preview them on the page thumbnails.
import type { PageNumbers } from "../bindings/PageNumbers";

/**
 * Pages of a document of `count` pages chosen by `text` ("1-3, 5, 8-"):
 * every page when empty, pages beyond the document ignored; `null` when the
 * text is not valid.
 */
export function chosenPages(text: string | null | undefined, count: number): boolean[] | null {
  const parts = (text ?? "")
    .split(/[,;]/)
    .map((p) => p.trim())
    .filter(Boolean);
  if (parts.length === 0) return Array.from({ length: count }, () => true);
  const chosen = Array.from({ length: count }, () => false);
  for (const part of parts) {
    const m = part.match(/^(\d*)\s*(-?)\s*(\d*)$/);
    if (!m || (!m[1] && !m[3])) return null;
    const first = m[1] ? Number(m[1]) : 1;
    const last = m[3] ? Number(m[3]) : m[2] ? Infinity : first;
    if (first < 1 || first > last) return null;
    for (let p = first; p <= Math.min(last, count); p++) chosen[p - 1] = true;
  }
  return chosen;
}

/** The number each page of a document of `count` pages shows (`null`: none). */
export function pageLabels(numbers: PageNumbers, count: number): (string | null)[] {
  // Skipped pages still count (a range that is not valid skips nothing).
  const skipped = numbers.skip?.trim() ? chosenPages(numbers.skip, count) : null;
  const first = Math.max(1, numbers.first);
  const total = numbers.start + count - first;
  return Array.from({ length: count }, (_, i) => {
    const position = i + 1;
    if (position < first || skipped?.[i]) return null;
    const n = numbers.start + position - first;
    if (numbers.format === "ofTotal") return `${n} / ${total}`;
    if (numbers.format === "word" && numbers.word?.trim()) return `${numbers.word.trim()} ${n}`;
    return String(n);
  });
}

/** Distance of page numbers from the edges, in points (as in the core). */
export const NUMBER_MARGIN = 28;
/** Distance of a watermark from the edges when it is not centered. */
export const WATERMARK_MARGIN = 36;

/**
 * Where a stamp of `box` size goes on a page shown `page` wide and high, as
 * the core places it: its center (from the top left, in points) and how much
 * smaller it is made to fit the page.
 */
export function placeBox(
  anchor: string,
  page: { width: number; height: number },
  box: { width: number; height: number },
  margin: number,
  angle: number,
): { x: number; y: number; fit: number } {
  const rad = (angle * Math.PI) / 180;
  const [sin, cos] = [Math.abs(Math.sin(rad)), Math.abs(Math.cos(rad))];
  let turned = { width: box.width * cos + box.height * sin, height: box.width * sin + box.height * cos };
  const room = Math.max(margin, 18);
  const fit = Math.min(1, Math.max(0.01, Math.min((page.width - 2 * room) / turned.width, (page.height - 2 * room) / turned.height)));
  turned = { width: turned.width * fit, height: turned.height * fit };
  const x = /Left$|^left$/.test(anchor)
    ? margin + turned.width / 2
    : /Right$|^right$/.test(anchor)
      ? page.width - margin - turned.width / 2
      : page.width / 2;
  const y = /^top/.test(anchor)
    ? margin + turned.height / 2
    : /^bottom/.test(anchor)
      ? page.height - margin - turned.height / 2
      : page.height / 2;
  return { x, y, fit };
}
