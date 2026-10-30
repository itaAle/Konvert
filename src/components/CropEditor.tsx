import { RectangleHorizontal, RectangleVertical } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

import type { CropRect } from "../bindings/CropRect";
import { t } from "../i18n";
import { backend } from "../lib/api";
import {
  MIN_CROP,
  clampCrop,
  cropWithRatio,
  isWhole,
  ratioValue,
  turnedSize,
  type CropRatio,
  type EditState,
  type Size,
} from "../state/edit";
import { Segmented } from "./ui/Segmented";

interface CropEditorProps {
  /** The photo, for its preview. */
  path: string;
  /** Its size as shown (before the turn chosen here). */
  size: Size;
  value: EditState;
  onChange: (value: EditState) => void;
}

/** Largest preview, in CSS pixels. */
const BOX = { width: 560, height: 360 };
/** Thumbnail asked from the backend. */
const PREVIEW = 512;

type Drag = { kind: "move" | "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw"; x: number; y: number; start: CropRect };

const WHOLE: CropRect = { x: 0, y: 0, width: 1, height: 1 };
const RATIOS: CropRatio[] = ["free", "original", "1:1", "4:3", "3:2", "16:9"];

/**
 * The photo as it will come out (turned and mirrored) with a frame to keep:
 * dragged by its middle or its handles, moved with the arrow keys (Shift:
 * resized), with free or fixed proportions.
 */
export function CropEditor({ path, size, value, onChange }: CropEditorProps) {
  const [src, setSrc] = useState<string | null>(null);
  const drag = useRef<Drag | null>(null);
  const turned = turnedSize(size, value.rotation);
  const scale = Math.min(BOX.width / turned.width, BOX.height / turned.height);
  const shown = { width: Math.round(turned.width * scale), height: Math.round(turned.height * scale) };
  const quarter = value.rotation % 180 === 90;
  const picture = quarter ? { width: shown.height, height: shown.width } : shown;
  const crop = value.crop ?? WHOLE;
  const ratio = ratioValue(value.cropRatio, value.cropPortrait, turned);
  // Width over height of the frame in fractions of the picture.
  const fractionRatio = ratio == null ? null : (ratio * turned.height) / turned.width;

  useEffect(() => {
    let alive = true;
    backend
      .pictureThumbnail(path, PREVIEW)
      .then((url) => alive && setSrc(url))
      .catch(() => alive && setSrc(null));
    return () => {
      alive = false;
    };
  }, [path]);

  const commit = (next: CropRect | null, patch: Partial<EditState> = {}) =>
    onChange({ ...value, ...patch, crop: next == null || isWhole(next) ? null : clampCrop(next) });

  /** The frame dragged by `kind`, `dx`×`dy` fractions away from where it started. */
  const dragged = (kind: Drag["kind"], start: CropRect, dx: number, dy: number): CropRect => {
    if (kind === "move") return clampCrop({ ...start, x: start.x + dx, y: start.y + dy });
    let { x, y, width, height } = start;
    const right = x + width;
    const bottom = y + height;
    if (kind.includes("w")) x = Math.min(Math.max(0, x + dx), right - MIN_CROP);
    if (kind.includes("e")) width = Math.min(Math.max(MIN_CROP, width + dx), 1 - x);
    if (kind.includes("w")) width = right - x;
    if (kind.includes("n")) y = Math.min(Math.max(0, y + dy), bottom - MIN_CROP);
    if (kind.includes("s")) height = Math.min(Math.max(MIN_CROP, height + dy), 1 - y);
    if (kind.includes("n")) height = bottom - y;
    if (fractionRatio != null) {
      // Fixed proportions (corners only): the width leads, within the picture.
      const roomW = kind.includes("w") ? right : 1 - start.x;
      const roomH = kind.includes("n") ? bottom : 1 - start.y;
      width = Math.max(MIN_CROP, Math.min(width, roomW, roomH * fractionRatio));
      height = width / fractionRatio;
      x = kind.includes("w") ? right - width : start.x;
      y = kind.includes("n") ? bottom - height : start.y;
    }
    return { x, y, width, height };
  };

  const onPointerDown = (e: PointerEvent, kind: Drag["kind"]) => {
    e.preventDefault();
    e.stopPropagation();
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
    drag.current = { kind, x: e.clientX, y: e.clientY, start: crop };
  };
  const onPointerMove = (e: PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const next = dragged(d.kind, d.start, (e.clientX - d.x) / shown.width, (e.clientY - d.y) / shown.height);
    commit(next);
  };
  const onPointerUp = () => {
    drag.current = null;
  };

  const onKeyDown = (e: KeyboardEvent) => {
    const step = 0.01;
    const delta = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] }[e.key];
    if (!delta) return;
    e.preventDefault();
    const [dx, dy] = delta;
    commit(e.shiftKey ? dragged("se", crop, dx, dy) : dragged("move", crop, dx, dy));
  };

  const chooseRatio = (cropRatio: CropRatio, portrait = value.cropPortrait) => {
    const r = ratioValue(cropRatio, portrait, turned);
    commit(r == null ? value.crop : cropWithRatio(r, turned, value.crop), { cropRatio, cropPortrait: portrait });
  };

  const pixels = {
    width: Math.max(1, Math.round(crop.width * turned.width)),
    height: Math.max(1, Math.round(crop.height * turned.height)),
  };
  const corners: Drag["kind"][] = ["nw", "ne", "sw", "se"];
  const edges: Drag["kind"][] = fractionRatio == null ? ["n", "s", "e", "w"] : [];
  const handle = (kind: Drag["kind"]) => {
    const left = kind.includes("w") ? "0%" : kind.includes("e") ? "100%" : "50%";
    const top = kind.includes("n") ? "0%" : kind.includes("s") ? "100%" : "50%";
    const cursor = { n: "ns", s: "ns", e: "ew", w: "ew", ne: "nesw", sw: "nesw", nw: "nwse", se: "nwse", move: "move" }[kind];
    return (
      <span
        key={kind}
        aria-hidden
        onPointerDown={(e) => onPointerDown(e, kind)}
        style={{ left, top, cursor: `${cursor}-resize`, touchAction: "none" }}
        className="absolute size-4 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white bg-accent shadow"
      />
    );
  };

  return (
    <div className="flex flex-col gap-3 rounded-[14px] bg-surface px-4 py-3 shadow-card">
      <div className="flex items-center gap-2">
        <div className="min-w-0 flex-1">
          <Segmented<CropRatio>
            label={t("edit.ratio")}
            value={value.cropRatio}
            options={RATIOS.map((r) => ({
              value: r,
              label: r === "free" ? t("edit.free") : r === "original" ? t("edit.ratioOriginal") : r,
            }))}
            onChange={(r) => chooseRatio(r)}
          />
        </div>
        {!["free", "original", "1:1"].includes(value.cropRatio) && (
          <button
            type="button"
            title={t("edit.swapRatio")}
            aria-label={t("edit.swapRatio")}
            onClick={() => chooseRatio(value.cropRatio, !value.cropPortrait)}
            className="flex size-9 shrink-0 items-center justify-center rounded-[10px] bg-fill text-label hover:bg-fill-strong"
          >
            {value.cropPortrait ? <RectangleVertical size={17} aria-hidden /> : <RectangleHorizontal size={17} aria-hidden />}
          </button>
        )}
      </div>

      <div className="flex justify-center">
        <div
          className="relative select-none"
          style={{ width: shown.width, height: shown.height }}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
        >
          {/* The photo and the shade around the frame, clipped; the frame and its handles are not. */}
          <div className="pointer-events-none absolute inset-0 overflow-hidden rounded-[8px] bg-fill">
            {src && (
              <img
                src={src}
                alt=""
                draggable={false}
                className="absolute max-w-none"
                style={{
                  width: picture.width,
                  height: picture.height,
                  left: (shown.width - picture.width) / 2,
                  top: (shown.height - picture.height) / 2,
                  transform: `scale(${value.flipHorizontal ? -1 : 1}, ${value.flipVertical ? -1 : 1}) rotate(${value.rotation}deg)`,
                }}
              />
            )}
            <div
              className="absolute"
              style={{
                left: crop.x * shown.width,
                top: crop.y * shown.height,
                width: crop.width * shown.width,
                height: crop.height * shown.height,
                boxShadow: "0 0 0 9999px rgb(0 0 0 / 0.45)",
              }}
            />
          </div>
          <div
            role="group"
            tabIndex={0}
            aria-label={t("edit.cropArea", { w: pixels.width, h: pixels.height })}
            onKeyDown={onKeyDown}
            onPointerDown={(e) => onPointerDown(e, "move")}
            className="absolute border-2 border-white outline-none focus-visible:border-accent"
            style={{
              left: crop.x * shown.width,
              top: crop.y * shown.height,
              width: crop.width * shown.width,
              height: crop.height * shown.height,
              cursor: "move",
              touchAction: "none",
            }}
          >
            {[...corners, ...edges].map(handle)}
          </div>
        </div>
      </div>

      <div className="flex items-center justify-between gap-3">
        <p className="px-1 text-[12px] leading-snug text-label-secondary">
          {value.crop ? t("edit.cropSize", { w: pixels.width, h: pixels.height }) : t("edit.cropHint")}
        </p>
        {value.crop && (
          <button
            type="button"
            onClick={() => commit(null, { cropRatio: "free" })}
            className="shrink-0 rounded-full px-2 py-1 text-[13px] font-medium text-accent hover:bg-accent-soft"
          >
            {t("edit.noCrop")}
          </button>
        )}
      </div>
    </div>
  );
}
