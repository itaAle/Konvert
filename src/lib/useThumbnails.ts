// Thumbnails of the pages in the page editor, drawn by the backend a few at a
// time and only for the pages that come into view: pages of PDFs, and photos
// (one page each). Nothing here leaves the computer: each batch is one local
// IPC call.
import { useCallback, useEffect, useRef, useState } from "react";

import type { Thumbnail } from "../bindings/Thumbnail";
import { pageId } from "../state/pages";
import { backend } from "./api";

/** Pages drawn per call (the backend accepts up to 48). */
const BATCH = 12;

/** A file whose pages are shown. */
export interface ThumbnailSource {
  key: number;
  path: string;
  /** A photo: its size in pixels, as shown (one page). */
  picture: { width: number; height: number } | null;
}

export function useThumbnails(sources: ThumbnailSource[], size: number) {
  const [thumbnails, setThumbnails] = useState<ReadonlyMap<string, Thumbnail>>(new Map());
  // Pages in view and not drawn yet, pages already asked for, one call at a time.
  const wanted = useRef(new Map<string, { file: number; number: number }>());
  const asked = useRef(new Set<string>());
  const busy = useRef(false);
  const files = useRef(new Map<number, ThumbnailSource>());
  files.current = new Map(sources.map((s) => [s.key, s]));

  useEffect(() => {
    wanted.current.clear();
    asked.current.clear();
    setThumbnails(new Map());
  }, [size]);

  const pump = useCallback(() => {
    if (busy.current) return;
    const first = wanted.current.values().next().value;
    if (!first) return;
    const source = files.current.get(first.file);
    // The pages of one file at a time.
    const batch = [...wanted.current.values()].filter((p) => p.file === first.file).slice(0, source?.picture ? 1 : BATCH);
    batch.forEach((p) => {
      wanted.current.delete(pageId(p));
      asked.current.add(pageId(p));
    });
    if (!source) {
      pump();
      return;
    }
    busy.current = true;
    const picture = source.picture;
    const drawn: Promise<Thumbnail[]> = picture
      ? backend
          .pictureThumbnail(source.path, size)
          .then((dataUrl) => [{ number: 1, widthPt: picture.width, heightPt: picture.height, dataUrl }])
      : backend.pdfThumbnails(
          source.path,
          batch.map((p) => p.number),
          size,
        );
    drawn
      // A page that cannot be drawn shows a placeholder instead of a picture.
      .catch((): Thumbnail[] => batch.map(({ number }) => ({ number, widthPt: 0, heightPt: 0, dataUrl: "" })))
      .then((pages) => {
        setThumbnails((previous) => {
          const next = new Map(previous);
          pages.forEach((t) => next.set(pageId({ file: source.key, number: t.number }), t));
          return next;
        });
      })
      .finally(() => {
        busy.current = false;
        pump();
      });
  }, [size]);

  /** A page came into view (`true`) or left it before being drawn (`false`). */
  const visible = useCallback(
    (page: { file: number; number: number }, inView: boolean) => {
      const id = pageId(page);
      if (!inView) {
        wanted.current.delete(id);
        return;
      }
      if (asked.current.has(id)) return;
      wanted.current.set(id, page);
      // Gather the pages that appear together into one call.
      setTimeout(pump, 0);
    },
    [pump],
  );

  return { thumbnails, visible };
}
