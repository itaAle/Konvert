import { useEffect, useId, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

import { t } from "../i18n";
import { backend } from "../lib/api";
import { MIN_LENGTH, clampTrim, formatTime, parseTime, trimLength, type TrimState } from "../state/trim";
import { Segmented } from "./ui/Segmented";

interface TrimPanelProps {
  path: string;
  /** Length of the file, in seconds. */
  duration: number;
  /** A video (frames are shown) or audio (its waveform). */
  video: boolean;
  value: TrimState | null;
  onChange: (value: TrimState | null) => void;
  /** Precise when the user did not choose (a conversion encodes anyway). */
  preciseByDefault: boolean;
}

/** Width of the waveform asked from the backend. */
const WAVE = { width: 600, height: 64 };
/** Width of the frame previews. */
const FRAME = 320;

/** Waits until a value stops changing for a moment. */
function useSettled<T>(value: T, ms: number): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), ms);
    return () => clearTimeout(timer);
  }, [value, ms]);
  return settled;
}

/** A frame of the video at `at` seconds (`null` while it is drawn or if it fails). */
function useFrame(path: string, at: number, enabled: boolean): string | null {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    backend
      .videoFrame(path, at, FRAME)
      .then((u) => alive && setUrl(u))
      .catch(() => alive && setUrl(null));
    return () => {
      alive = false;
    };
  }, [path, at, enabled]);
  return url;
}

function TimeField({ label, value, onCommit }: { label: string; value: number; onCommit: (seconds: number) => void }) {
  const id = useId();
  const [text, setText] = useState(formatTime(value));
  const [wrong, setWrong] = useState(false);
  useEffect(() => {
    setText(formatTime(value));
    setWrong(false);
  }, [value]);
  const commit = () => {
    const seconds = parseTime(text);
    if (seconds == null) return setWrong(true);
    setWrong(false);
    onCommit(seconds);
  };
  return (
    <div className="flex flex-1 flex-col gap-1">
      <label htmlFor={id} className="px-1 text-[12px] font-medium text-label-secondary">
        {label}
      </label>
      <input
        id={id}
        value={text}
        inputMode="decimal"
        spellCheck={false}
        aria-invalid={wrong}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => e.key === "Enter" && commit()}
        className={`h-9 w-full rounded-[10px] bg-fill px-3 text-[14px] text-label tabular-nums transition-colors hover:bg-fill-strong focus:bg-fill-strong ${
          wrong ? "ring-2 ring-red-500/70" : ""
        }`}
      />
    </div>
  );
}

/**
 * The part of the file to keep: two handles on a bar (the waveform of an
 * audio file), start and end typed as hh:mm:ss.mmm, the frames at both ends
 * of a video, fast (no encoding, from the nearest key frame) or precise.
 */
export function TrimPanel({ path, duration, video, value, onChange, preciseByDefault }: TrimPanelProps) {
  const bar = useRef<HTMLDivElement>(null);
  const dragging = useRef<"start" | "end" | null>(null);
  const [wave, setWave] = useState<string | null>(null);
  const start = value?.start ?? 0;
  const end = value?.end ?? duration;
  const precise = value?.precise ?? preciseByDefault;
  const settledStart = useSettled(start, 250);
  const settledEnd = useSettled(end, 250);
  // The last frame starts a moment before the end.
  const startFrame = useFrame(path, settledStart, video);
  const endFrame = useFrame(path, Math.max(0, settledEnd - 0.05), video);

  useEffect(() => {
    if (video) return;
    let alive = true;
    backend
      .audioWaveform(path, WAVE.width, WAVE.height)
      .then((u) => alive && setWave(u))
      .catch(() => alive && setWave(null));
    return () => {
      alive = false;
    };
  }, [path, video]);

  const set = (patch: Partial<TrimState>) => {
    const next = clampTrim({ start, end, precise: value?.precise ?? null, ...patch }, duration);
    onChange(next.start <= 0.0005 && next.end == null && next.precise == null ? null : next);
  };

  const at = (clientX: number) => {
    const rect = bar.current!.getBoundingClientRect();
    return Math.min(Math.max((clientX - rect.left) / rect.width, 0), 1) * duration;
  };
  const onPointerDown = (e: PointerEvent, handle: "start" | "end") => {
    e.preventDefault();
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
    dragging.current = handle;
  };
  const onPointerMove = (e: PointerEvent) => {
    const handle = dragging.current;
    if (!handle) return;
    const seconds = at(e.clientX);
    set(handle === "start" ? { start: Math.min(seconds, end - MIN_LENGTH) } : { end: Math.max(seconds, start + MIN_LENGTH) });
  };
  const onKey = (e: KeyboardEvent, handle: "start" | "end") => {
    const step = e.shiftKey ? 1 : 0.1;
    const delta = { ArrowLeft: -step, ArrowDown: -step, ArrowRight: step, ArrowUp: step }[e.key];
    if (delta === undefined) return;
    e.preventDefault();
    set(handle === "start" ? { start: start + delta } : { end: end + delta });
  };

  const left = (start / duration) * 100;
  const right = (end / duration) * 100;
  const handle = (which: "start" | "end", position: number, seconds: number) => (
    <span
      role="slider"
      tabIndex={0}
      aria-label={t(which === "start" ? "trim.start" : "trim.end")}
      aria-valuemin={0}
      aria-valuemax={Math.round(duration * 10) / 10}
      aria-valuenow={Math.round(seconds * 10) / 10}
      aria-valuetext={formatTime(seconds)}
      onPointerDown={(e) => onPointerDown(e, which)}
      onKeyDown={(e) => onKey(e, which)}
      style={{ left: `${position}%`, touchAction: "none" }}
      className="absolute top-1/2 h-[calc(100%+10px)] w-3 -translate-x-1/2 -translate-y-1/2 cursor-ew-resize rounded-full border-2 border-white bg-accent shadow outline-none focus-visible:ring-2 focus-visible:ring-accent"
    />
  );

  return (
    <div className="flex flex-col gap-3 rounded-[14px] bg-surface px-4 py-3 shadow-card">
      <div
        ref={bar}
        className="relative h-16 select-none"
        onPointerMove={onPointerMove}
        onPointerUp={() => (dragging.current = null)}
        onPointerCancel={() => (dragging.current = null)}
      >
        <div className="absolute inset-0 overflow-hidden rounded-[8px] bg-fill">
          {wave && <img src={wave} alt="" className="pointer-events-none size-full object-fill opacity-80" />}
          <div className="absolute inset-y-0 left-0 bg-black/45" style={{ width: `${left}%` }} />
          <div className="absolute inset-y-0 right-0 bg-black/45" style={{ width: `${100 - right}%` }} />
          <div className="absolute inset-y-0 border-y-2 border-accent" style={{ left: `${left}%`, width: `${right - left}%` }} />
        </div>
        {handle("start", left, start)}
        {handle("end", right, end)}
      </div>

      {video && (
        <div className="grid grid-cols-2 gap-3">
          {[startFrame, endFrame].map((frame, i) => (
            <div key={i} className="aspect-video overflow-hidden rounded-[8px] bg-fill">
              {frame && <img src={frame} alt={t(i === 0 ? "trim.startFrame" : "trim.endFrame")} className="size-full object-contain" />}
            </div>
          ))}
        </div>
      )}

      <div className="flex items-end gap-3">
        <TimeField label={t("trim.start")} value={start} onCommit={(s) => set({ start: s })} />
        <TimeField label={t("trim.end")} value={end} onCommit={(s) => set({ end: s })} />
      </div>
      <p className="px-1 text-[12px] text-label-secondary" aria-live="polite">
        {t("trim.length", { length: formatTime(trimLength({ start, end, precise }, duration)) })}
      </p>

      <Segmented<"fast" | "precise">
        label={t("trim.mode")}
        value={precise ? "precise" : "fast"}
        options={[
          { value: "fast", label: t("trim.fast") },
          { value: "precise", label: t("trim.precise") },
        ]}
        onChange={(mode) => set({ precise: mode === "precise" })}
      />
      <p className="-mt-1 px-1 text-[12px] leading-snug text-label-secondary">
        {t(precise ? "trim.preciseHint" : "trim.fastHint")}
      </p>
    </div>
  );
}
