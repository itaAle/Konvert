import { motion, useReducedMotion } from "motion/react";
import { FileX2, RotateCcw, RotateCw, Scissors, Trash2, Undo2 } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";

import type { PdfSplit } from "../bindings/PdfSplit";
import type { Thumbnail } from "../bindings/Thumbnail";
import { intlLocale, t } from "../i18n";
import { spring } from "../lib/motion";
import { useThumbnails, type ThumbnailSource } from "../lib/useThumbnails";
import {
  everyPositions,
  isEdited,
  keepOnly,
  keptCount,
  movePage,
  pageId,
  parseRange,
  reconcile,
  rotateAll,
  rotatePage,
  toggleCut,
  toggleRemoved,
  type EditorPage,
} from "../state/pages";
import { Button } from "./ui/Button";

/** Pages per row; arrow keys up and down move by a row. */
const COLUMNS = 5;
/** Longest side of a thumbnail in pixels (sharp at twice the displayed size). */
const THUMBNAIL_SIZE = 224;
/** Beyond this many pages, moving pages is not animated. */
const ANIMATED_PAGES = 150;
/** Colors telling the files apart (system colors, readable in both themes). */
const FILE_COLORS = ["#0a84ff", "#ff9f0a", "#30d158", "#bf5af2", "#ff375f", "#40c8e0", "#ffcc00", "#ac8e68"];

/** A file whose pages are in the editor. */
export interface EditorSource extends ThumbnailSource {
  name: string;
  /** Its pages (a photo is one page). */
  count: number;
  /** Size of its first page in points, shown until each thumbnail arrives. */
  firstPage: { widthPt: number; heightPt: number };
}

interface PageEditorProps {
  /** The files, in list order: one PDF, or PDFs and photos combined. */
  sources: EditorSource[];
  pages: EditorPage[] | null;
  cuts: string[];
  split: PdfSplit;
  splitEvery: number;
  /** `pages` is `null` when nothing is changed. */
  onChange: (pages: EditorPage[] | null, cuts: string[]) => void;
}

interface Drag {
  from: number;
  pointerId: number;
  startX: number;
  startY: number;
  x: number;
  y: number;
  active: boolean;
  /** Index the page would be inserted before (`pages.length`: at the end). */
  before: number | null;
}

/**
 * Pages of a PDF, or of several files combined into one, as thumbnails: drag
 * to move a page (also between files), buttons (or keys) to turn it or take
 * it out, scissors between pages to choose where a split starts a new
 * document. Thumbnails are drawn locally, only for the pages in view.
 */
export function PageEditor({ sources, pages, cuts, split, splitEvery, onChange }: PageEditorProps) {
  const list = useMemo(() => reconcile(pages, sources), [pages, sources]);
  const { thumbnails, visible } = useThumbnails(sources, THUMBNAIL_SIZE);
  const reduceMotion = useReducedMotion();
  const hintId = useId();
  const keepId = useId();
  const [focus, setFocus] = useState(0);
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragRef = useRef<Drag | null>(null);
  const [keep, setKeep] = useState("");
  const [keepInvalid, setKeepInvalid] = useState(false);
  const number = (n: number) => new Intl.NumberFormat(intlLocale()).format(n);
  const many = sources.length > 1;
  const byKey = useMemo(() => new Map(sources.map((s, i) => [s.key, { ...s, color: FILE_COLORS[i % FILE_COLORS.length] }])), [sources]);

  const kept = keptCount(list);
  const update = (next: EditorPage[], nextCuts = cuts) => onChange(isEdited(next, sources) ? next : null, nextCuts);

  // Where a new document starts: after these pages.
  const cutAfter = useMemo(() => {
    const keptPages = list.filter((p) => !p.removed);
    const positions =
      split === "every"
        ? everyPositions(keptPages.length, splitEvery)
        : split === "after"
          ? keptPages.flatMap((p, i) => (cuts.includes(pageId(p)) && i < keptPages.length - 1 ? [i + 1] : []))
          : [];
    return new Set(positions.map((position) => pageId(keptPages[position - 1])));
  }, [list, cuts, split, splitEvery]);
  // First page of each new document, with its number ("PDF 2").
  const partStarts = useMemo(() => {
    const starts = new Map<string, number>();
    if (cutAfter.size === 0) return starts;
    let part = 1;
    let starting = true;
    for (const page of list) {
      if (page.removed) continue;
      if (starting) starts.set(pageId(page), part++);
      starting = cutAfter.has(pageId(page));
    }
    return starts;
  }, [list, cutAfter]);
  const last = [...list].reverse().find((p) => !p.removed);
  const lastKept = last ? pageId(last) : null;

  // --- Thumbnails of the pages in view ----------------------------------------------
  const elements = useRef(new Map<string, HTMLDivElement>());
  const refs = useRef(new Map<string, (el: HTMLDivElement | null) => void>());
  const observer = useRef<IntersectionObserver | null>(null);
  useEffect(() => {
    const io = new IntersectionObserver(
      (entries) =>
        entries.forEach((e) => {
          const { file, page } = (e.target as HTMLElement).dataset;
          visible({ file: Number(file), number: Number(page) }, e.isIntersecting);
        }),
      { rootMargin: "320px 0px" },
    );
    observer.current = io;
    elements.current.forEach((el) => io.observe(el));
    return () => io.disconnect();
  }, [visible]);
  const refFor = (id: string) => {
    let ref = refs.current.get(id);
    if (!ref) {
      ref = (el) => {
        const old = elements.current.get(id);
        if (old && old !== el) observer.current?.unobserve(old);
        if (el) {
          elements.current.set(id, el);
          observer.current?.observe(el);
        } else {
          elements.current.delete(id);
        }
      };
      refs.current.set(id, ref);
    }
    return ref;
  };
  /** Focuses a page, which will be at `index` once a move is applied. */
  const focusPage = (index: number, id: string) => {
    setFocus(index);
    requestAnimationFrame(() => elements.current.get(id)?.focus());
  };

  // Turns animate the short way round (270° → 0° turns a quarter, not three).
  const angles = useRef(new Map<string, { rotation: number; angle: number }>());
  const angleOf = (page: EditorPage) => {
    const shown = angles.current.get(pageId(page));
    if (!shown) {
      angles.current.set(pageId(page), { rotation: page.rotation, angle: page.rotation });
      return page.rotation;
    }
    if (shown.rotation !== page.rotation) {
      shown.angle += ((page.rotation - shown.rotation + 540) % 360) - 180;
      shown.rotation = page.rotation;
    }
    return shown.angle;
  };

  // --- Moving pages: pointer drag (HTML drag and drop is taken by file drops) -----
  const scroller = useRef<HTMLElement | null>(null);
  const frame = useRef(0);
  const insertionAt = (x: number, y: number) => {
    let best: { index: number; distance: number; after: boolean } | null = null;
    list.forEach((page, index) => {
      const rect = elements.current.get(pageId(page))?.getBoundingClientRect();
      if (!rect) return;
      const cx = rect.left + rect.width / 2;
      const cy = rect.top + rect.height / 2;
      const distance = Math.hypot(x - cx, (y - cy) * 1.5);
      if (!best || distance < best.distance) best = { index, distance, after: x > cx };
    });
    const found = best as { index: number; after: boolean } | null;
    return found ? found.index + (found.after ? 1 : 0) : null;
  };
  // While dragging near the top or bottom edge, the page list scrolls.
  const autoScroll = () => {
    cancelAnimationFrame(frame.current);
    const d = dragRef.current;
    const area = scroller.current;
    if (!d?.active || !area) return;
    const rect = area.getBoundingClientRect();
    const edge = 56;
    const speed = d.y < rect.top + edge ? -(rect.top + edge - d.y) / 4 : d.y > rect.bottom - edge ? (d.y - rect.bottom + edge) / 4 : 0;
    if (speed === 0) return;
    area.scrollBy(0, speed);
    const before = insertionAt(d.x, d.y);
    if (before !== d.before) {
      d.before = before;
      setDrag({ ...d });
    }
    frame.current = requestAnimationFrame(autoScroll);
  };
  useEffect(() => () => cancelAnimationFrame(frame.current), []);

  const onPointerDown = (e: PointerEvent<HTMLDivElement>, index: number) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    scroller.current = e.currentTarget.closest<HTMLElement>(".overflow-y-auto");
    dragRef.current = { from: index, pointerId: e.pointerId, startX: e.clientX, startY: e.clientY, x: e.clientX, y: e.clientY, active: false, before: null };
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const d = dragRef.current;
    if (!d || d.pointerId !== e.pointerId) return;
    if (!d.active && Math.hypot(e.clientX - d.startX, e.clientY - d.startY) < 6) return;
    d.active = true;
    d.x = e.clientX;
    d.y = e.clientY;
    d.before = insertionAt(e.clientX, e.clientY);
    setDrag({ ...d });
    autoScroll();
  };
  const endDrag = (e: PointerEvent<HTMLDivElement>, drop: boolean) => {
    const d = dragRef.current;
    if (!d || d.pointerId !== e.pointerId) return;
    dragRef.current = null;
    cancelAnimationFrame(frame.current);
    setDrag(null);
    if (!drop || !d.active || d.before == null) return;
    const to = d.before > d.from ? d.before - 1 : d.before;
    if (to !== d.from) update(movePage(list, d.from, to));
    focusPage(to, pageId(list[d.from]));
  };

  // --- Keyboard: the same actions as the pointer -------------------------------------
  const toggleCutAfter = (page: EditorPage) => onChange(pages, toggleCut(cuts, pageId(page)));
  const onKeyDown = (e: KeyboardEvent, index: number) => {
    const page = list[index];
    const step = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -COLUMNS, ArrowDown: COLUMNS }[e.key];
    if (step !== undefined || e.key === "Home" || e.key === "End") {
      e.preventDefault();
      const target =
        e.key === "Home" ? 0 : e.key === "End" ? list.length - 1 : Math.min(list.length - 1, Math.max(0, index + step!));
      const moving = (e.ctrlKey || e.metaKey) && target !== index;
      if (moving) update(movePage(list, index, target));
      // A moved page keeps the focus; otherwise it goes to the page there.
      focusPage(target, pageId(moving ? page : list[target]));
    } else if (e.key === "r" || e.key === "R") {
      e.preventDefault();
      if (!page.removed) update(rotatePage(list, index, e.shiftKey ? -90 : 90));
    } else if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      update(toggleRemoved(list, index));
    } else if ((e.key === "s" || e.key === "S") && split === "after" && !page.removed && pageId(page) !== lastKept) {
      e.preventDefault();
      toggleCutAfter(page);
    }
  };

  const single = sources.length === 1 ? sources[0] : null;
  const applyKeep = () => {
    const numbers = parseRange(keep, single?.count ?? 0);
    setKeepInvalid(numbers == null);
    if (numbers) update(keepOnly(list, numbers));
  };

  const animate = !reduceMotion && list.length <= ANIMATED_PAGES;
  const dragged = drag?.active ? list[drag.from] : null;
  const fallback = (page: EditorPage) => {
    const source = byKey.get(page.file);
    if (source?.picture) return { widthPt: source.picture.width, heightPt: source.picture.height };
    return source?.firstPage ?? { widthPt: 595, heightPt: 842 };
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-1 px-1">
        <p className="min-w-0 flex-1 truncate text-[13px] text-label-secondary" aria-live="polite">
          {kept === list.length
            ? t("pdf.pages", { n: number(list.length) })
            : t("pdf.pagesKept", { kept: number(kept), n: number(list.length) })}
        </p>
        <IconButton label={t("pdf.rotateAllLeft")} onClick={() => update(rotateAll(list, -90))}>
          <RotateCcw size={16} aria-hidden />
        </IconButton>
        <IconButton label={t("pdf.rotateAllRight")} onClick={() => update(rotateAll(list, 90))}>
          <RotateCw size={16} aria-hidden />
        </IconButton>
        <Button variant="plain" size="sm" disabled={!isEdited(list, sources) && cuts.length === 0} onClick={() => onChange(null, [])}>
          {t("pdf.restoreAll")}
        </Button>
      </div>

      {many && (
        <ul aria-label={t("pdf.files")} className="flex flex-wrap gap-x-4 gap-y-1 px-1">
          {[...byKey.values()].map((source) => (
            <li key={source.key} className="flex min-w-0 max-w-full items-center gap-1.5 text-[12px] text-label-secondary">
              <span aria-hidden className="size-2 shrink-0 rounded-full" style={{ backgroundColor: source.color }} />
              <span className="truncate">{source.name}</span>
            </li>
          ))}
        </ul>
      )}

      <ul
        aria-label={t("pdf.editTitle")}
        aria-describedby={hintId}
        className={`grid grid-cols-5 gap-x-3 gap-y-2 ${drag?.active ? "cursor-grabbing" : ""}`}
      >
        {list.map((page, index) => {
          const id = pageId(page);
          const source = byKey.get(page.file);
          const photo = source?.picture != null;
          const thumbnail = thumbnails.get(id);
          const cut = cutAfter.has(id);
          const part = partStarts.get(id);
          const canCut = split === "after" && !page.removed && id !== lastKept;
          const insertBefore = drag?.active && drag.before === index && drag.before !== drag.from && drag.before !== drag.from + 1;
          const insertAfter = drag?.active && index === list.length - 1 && drag.before === list.length && drag.from !== list.length - 1;
          const name = photo
            ? t("pdf.photoOf", { file: source!.name })
            : many
              ? t("pdf.pageOf", { n: page.number, file: source?.name ?? "" })
              : t("pdf.page", { n: page.number });
          const label = [
            name,
            page.rotation ? t("pdf.pageTurned", { n: page.rotation }) : null,
            page.removed ? t("pdf.pageRemoved") : null,
            cut ? t("pdf.pageCut") : null,
          ]
            .filter(Boolean)
            .join(", ");
          return (
            <motion.li key={id} layout={animate ? "position" : false} transition={spring} className="relative min-w-0">
              <div
                ref={refFor(id)}
                data-file={page.file}
                data-page={page.number}
                role="group"
                aria-label={label}
                aria-describedby={hintId}
                title={many ? source?.name : undefined}
                tabIndex={index === focus ? 0 : -1}
                onFocus={() => setFocus(index)}
                onKeyDown={(e) => onKeyDown(e, index)}
                onPointerDown={(e) => onPointerDown(e, index)}
                onPointerMove={onPointerMove}
                onPointerUp={(e) => endDrag(e, true)}
                onPointerCancel={(e) => endDrag(e, false)}
                className={`group relative aspect-square touch-none rounded-[12px] bg-fill transition-[opacity,background-color] hover:bg-fill-strong ${
                  dragged === page ? "opacity-30" : ""
                } ${drag?.active ? "" : "cursor-grab"}`}
              >
                <PageImage
                  thumbnail={thumbnail}
                  fallback={fallback(page)}
                  rotation={page.rotation}
                  angle={angleOf(page)}
                  removed={page.removed}
                  animate={!reduceMotion}
                />
                {part != null && (
                  <span className="absolute top-1.5 left-1.5 rounded-full bg-accent px-1.5 py-px text-[10px] font-semibold text-white">
                    PDF {part}
                  </span>
                )}
                <div
                  className={`absolute inset-x-0 bottom-1.5 flex justify-center gap-1 transition-opacity ${
                    page.removed ? "opacity-100" : "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100"
                  } ${drag?.active ? "invisible" : ""}`}
                >
                  {!page.removed && (
                    <>
                      <PageButton label={t("pdf.rotateLeft")} onClick={() => update(rotatePage(list, index, -90))}>
                        <RotateCcw size={14} aria-hidden />
                      </PageButton>
                      <PageButton label={t("pdf.rotateRight")} onClick={() => update(rotatePage(list, index, 90))}>
                        <RotateCw size={14} aria-hidden />
                      </PageButton>
                    </>
                  )}
                  <PageButton
                    label={page.removed ? t("pdf.restore") : t("pdf.remove")}
                    disabled={!page.removed && kept <= 1}
                    onClick={() => update(toggleRemoved(list, index))}
                  >
                    {page.removed ? <Undo2 size={14} aria-hidden /> : <Trash2 size={14} aria-hidden />}
                  </PageButton>
                </div>
              </div>
              <p className="mt-1 flex items-center justify-center gap-1 truncate text-center text-[12px] text-label-secondary tabular-nums">
                {many && <span aria-hidden className="size-1.5 shrink-0 rounded-full" style={{ backgroundColor: source?.color }} />}
                <span className="truncate">
                  {photo ? t("pdf.photo") : page.number}
                  {page.rotation ? ` · ${page.rotation}°` : ""}
                </span>
              </p>
              {(insertBefore || insertAfter) && (
                <span
                  aria-hidden
                  className={`absolute top-0 bottom-5 w-[3px] rounded-full bg-accent ${insertAfter ? "-right-[7.5px]" : "-left-[7.5px]"}`}
                />
              )}
              {cut && <span aria-hidden className="absolute top-1 -right-[7px] bottom-6 w-[2px] rounded-full bg-accent" />}
              {(canCut || cut) && (
                <CutMark
                  active={cut}
                  editable={canCut}
                  label={
                    many
                      ? t(cut ? "pdf.uncutAfterPage" : "pdf.cutAfterPage", { page: name })
                      : t(cut ? "pdf.uncutAfter" : "pdf.cutAfter", { n: page.number })
                  }
                  onClick={() => toggleCutAfter(page)}
                />
              )}
            </motion.li>
          );
        })}
      </ul>

      <p id={hintId} className="px-1 text-[13px] text-label-secondary">
        {t("pdf.editHint")}
        <span className="sr-only"> {t("pdf.keyboardHint")}</span>
      </p>

      {single && single.count > 1 && (
        <div className="flex flex-col gap-1.5 px-1">
          <label htmlFor={keepId} className="px-1 text-[12px] font-medium text-label-secondary">
            {t("pdf.keep")}
          </label>
          <div className="flex items-center gap-2">
            <input
              id={keepId}
              type="text"
              inputMode="numeric"
              autoComplete="off"
              spellCheck={false}
              placeholder={t("pdf.keepPlaceholder")}
              value={keep}
              aria-invalid={keepInvalid}
              onChange={(e) => {
                setKeep(e.target.value);
                setKeepInvalid(false);
              }}
              onKeyDown={(e) => e.key === "Enter" && applyKeep()}
              className={`h-9 min-w-0 flex-1 rounded-[10px] bg-fill px-3 text-[14px] text-label transition-colors placeholder:text-label-tertiary hover:bg-fill-strong focus:bg-fill-strong ${
                keepInvalid ? "ring-2 ring-warning" : ""
              }`}
            />
            <Button size="md" disabled={keep.trim() === ""} onClick={applyKeep}>
              {t("pdf.keepApply")}
            </Button>
          </div>
          {keepInvalid && (
            <p className="px-1 text-[13px] text-warning" role="alert">
              {t("pdf.keepInvalid")}
            </p>
          )}
        </div>
      )}

      {dragged && drag && (
        <div
          aria-hidden
          className="pointer-events-none fixed z-50 w-[84px] -translate-x-1/2 -translate-y-1/2 rounded-[12px] bg-surface-raised shadow-card-lg"
          style={{ left: drag.x, top: drag.y }}
        >
          <div className="relative aspect-square">
            <PageImage
              thumbnail={thumbnails.get(pageId(dragged))}
              fallback={fallback(dragged)}
              rotation={dragged.rotation}
              angle={dragged.rotation}
              removed={dragged.removed}
              animate={false}
            />
          </div>
        </div>
      )}
    </div>
  );
}

/** The page drawn in its square, turned and fitted. */
function PageImage({
  thumbnail,
  fallback,
  rotation,
  angle,
  removed,
  animate,
}: {
  thumbnail: Thumbnail | undefined;
  fallback: { widthPt: number; heightPt: number };
  rotation: number;
  /** Angle actually shown (may go past 360° so turns animate the short way). */
  angle: number;
  removed: boolean;
  animate: boolean;
}) {
  const width = thumbnail?.widthPt || fallback.widthPt || 1;
  const height = thumbnail?.heightPt || fallback.heightPt || 1;
  // Sides once turned: the page must fit the square either way.
  const quarter = rotation % 180 !== 0;
  const scale = 100 / Math.max(quarter ? height : width, quarter ? width : height);
  const failed = thumbnail != null && thumbnail.dataUrl === "";
  return (
    <div className="absolute inset-2.5">
      <motion.div
        className="absolute top-1/2 left-1/2 overflow-hidden rounded-[2px] bg-white shadow-[0_1px_3px_rgb(0_0_0/0.18)]"
        style={{ width: `${width * scale}%`, height: `${height * scale}%`, x: "-50%", y: "-50%" }}
        initial={false}
        animate={{ rotate: angle }}
        transition={animate ? spring : { duration: 0 }}
      >
        {thumbnail && !failed ? (
          <img
            src={thumbnail.dataUrl}
            alt=""
            draggable={false}
            className={`size-full ${removed ? "opacity-35 grayscale" : ""}`}
          />
        ) : failed ? (
          <div className="flex size-full items-center justify-center text-[#8e8e93]" title={t("pdf.noPreview")}>
            <FileX2 size={18} aria-hidden />
          </div>
        ) : (
          <div className="size-full animate-pulse bg-[#f2f2f7]" />
        )}
      </motion.div>
    </div>
  );
}

function IconButton({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className="flex size-8 items-center justify-center rounded-full text-accent transition-colors hover:bg-accent-soft"
    >
      {children}
    </button>
  );
}

/** Small round button over a page (the same actions have keyboard shortcuts). */
function PageButton({
  label,
  onClick,
  disabled,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      tabIndex={-1}
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
      className="flex size-7 items-center justify-center rounded-full bg-surface-raised text-label shadow-card backdrop-blur-md transition-colors hover:text-accent disabled:text-label-tertiary"
    >
      {children}
    </button>
  );
}

/** Scissors in the gap after a page: a new document starts there. */
function CutMark({
  active,
  editable,
  label,
  onClick,
}: {
  active: boolean;
  editable: boolean;
  label: string;
  onClick: () => void;
}) {
  const look = `absolute top-[calc(50%-21px)] -right-[18px] z-10 flex size-[22px] items-center justify-center rounded-full transition-[opacity,color,background-color] ${
    active ? "bg-accent text-white shadow-card" : "bg-surface-raised text-label-tertiary opacity-60 shadow-card hover:text-accent hover:opacity-100"
  }`;
  if (!editable) {
    return (
      <span aria-hidden className={look}>
        <Scissors size={12} />
      </span>
    );
  }
  return (
    <button type="button" tabIndex={-1} aria-label={label} title={label} aria-pressed={active} onClick={onClick} className={look}>
      <Scissors size={12} aria-hidden />
    </button>
  );
}
