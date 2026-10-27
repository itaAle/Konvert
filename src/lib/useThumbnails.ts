// Thumbnails of the pages of a PDF, drawn by the backend a few at a time and
// only for the pages that come into view. Nothing here leaves the computer:
// each batch is one local IPC call.
import { useCallback, useEffect, useRef, useState } from "react";

import type { Thumbnail } from "../bindings/Thumbnail";
import { backend } from "./api";

/** Pages drawn per call (the backend accepts up to 48). */
const BATCH = 12;

export function useThumbnails(path: string, size: number) {
  const [thumbnails, setThumbnails] = useState<ReadonlyMap<number, Thumbnail>>(new Map());
  // Pages in view and not drawn yet, pages already asked for, one call at a time.
  const wanted = useRef(new Set<number>());
  const asked = useRef(new Set<number>());
  const busy = useRef(false);
  const current = useRef(path);

  useEffect(() => {
    current.current = path;
    wanted.current.clear();
    asked.current.clear();
    setThumbnails(new Map());
  }, [path, size]);

  const pump = useCallback(() => {
    if (busy.current) return;
    const batch = [...wanted.current].slice(0, BATCH);
    if (batch.length === 0) return;
    batch.forEach((n) => {
      wanted.current.delete(n);
      asked.current.add(n);
    });
    busy.current = true;
    const forPath = path;
    backend
      .pdfThumbnails(path, batch, size)
      // A page that cannot be drawn shows a placeholder instead of a picture.
      .catch((): Thumbnail[] => batch.map((number) => ({ number, widthPt: 0, heightPt: 0, dataUrl: "" })))
      .then((drawn) => {
        if (current.current !== forPath) return;
        setThumbnails((previous) => {
          const next = new Map(previous);
          drawn.forEach((t) => next.set(t.number, t));
          return next;
        });
      })
      .finally(() => {
        busy.current = false;
        pump();
      });
  }, [path, size]);

  /** A page came into view (`true`) or left it before being drawn (`false`). */
  const visible = useCallback(
    (number: number, inView: boolean) => {
      if (!inView) {
        wanted.current.delete(number);
        return;
      }
      if (asked.current.has(number)) return;
      wanted.current.add(number);
      // Gather the pages that appear together into one call.
      setTimeout(pump, 0);
    },
    [pump],
  );

  return { thumbnails, visible };
}
