import { motion } from "motion/react";
import { useId, useRef, type KeyboardEvent } from "react";

import type { FrameMode } from "../bindings/FrameMode";
import type { FrameOptions } from "../bindings/FrameOptions";
import { intlLocale, t, type MessageKey } from "../i18n";
import { formatDuration } from "../lib/format";
import { spring } from "../lib/motion";
import { Select } from "./ui/Select";

const MODES: { value: FrameMode; label: MessageKey }[] = [
  { value: "single", label: "frames.single" },
  { value: "sequence", label: "frames.sequence" },
];

/** Intervals offered for a series, in seconds; "all" means every frame. */
const INTERVALS = [0.5, 1, 2, 5, 10, 30, 60];
const ALL = "all";
/** A series starts with one picture per second. */
const DEFAULT_INTERVAL = 1;

interface FramesPanelProps {
  value: FrameOptions;
  onChange: (value: FrameOptions) => void;
  /** Shortest duration of the selected videos (`null`: unknown). */
  duration: number | null;
  /** Expected number of pictures for every video (`null`: unknown or not planned yet). */
  count: number | null;
}

const number = (n: number) => new Intl.NumberFormat(intlLocale(), { maximumFractionDigits: 1 }).format(n);

/** "1:05.5": position in the video, with tenths of a second when there are any. */
function formatPosition(seconds: number): string {
  const tenths = Math.round((seconds % 1) * 10) % 10;
  return tenths ? `${formatDuration(Math.floor(seconds))}.${tenths}` : formatDuration(seconds);
}

/** One frame at a chosen moment, or a series of pictures at regular intervals. */
export function FramesPanel({ value, onChange, duration, count }: FramesPanelProps) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const sliderId = useId();

  const setMode = (mode: FrameMode) => {
    if (mode === value.mode) return;
    onChange(
      mode === "single"
        ? { mode, atSecs: value.atSecs ?? null, everySecs: null }
        : { mode, atSecs: null, everySecs: DEFAULT_INTERVAL },
    );
  };
  const onKeyDown = (e: KeyboardEvent, index: number) => {
    const delta = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (delta === undefined) return;
    e.preventDefault();
    const next = (index + delta + MODES.length) % MODES.length;
    setMode(MODES[next].value);
    refs.current[next]?.focus();
  };

  const max = duration != null && duration > 0 ? duration : null;
  const at = value.atSecs ?? (max != null ? max / 2 : 0);

  return (
    <div>
      <div role="radiogroup" aria-label={t("frames.title")} className="flex rounded-[12px] bg-fill p-[3px]">
        {MODES.map((mode, index) => {
          const active = mode.value === value.mode;
          return (
            <button
              key={mode.value}
              ref={(el) => {
                refs.current[index] = el;
              }}
              type="button"
              role="radio"
              aria-checked={active}
              tabIndex={active ? 0 : -1}
              onClick={() => setMode(mode.value)}
              onKeyDown={(e) => onKeyDown(e, index)}
              className={`relative h-9 flex-1 rounded-[9px] text-[14px] font-medium transition-colors ${
                active ? "text-label" : "text-label-secondary hover:text-label"
              }`}
            >
              {active && (
                <motion.span
                  layoutId="frames-indicator"
                  transition={spring}
                  className="absolute inset-0 rounded-[9px] bg-surface-raised shadow-[0_1px_3px_rgb(0_0_0/0.12),0_1px_1px_rgb(0_0_0/0.04)]"
                />
              )}
              <span className="relative">{t(mode.label)}</span>
            </button>
          );
        })}
      </div>

      <div className="mt-3 px-1">
        {value.mode === "single" ? (
          max != null && (
            <div className="flex flex-col gap-1.5">
              <div className="flex items-baseline justify-between gap-3">
                <label htmlFor={sliderId} className="text-[12px] font-medium text-label-secondary">
                  {t("frames.moment")}
                </label>
                <span className="text-[13px] text-label tabular-nums" aria-hidden>
                  {value.atSecs == null ? `${t("frames.middle")} · ${formatPosition(at)}` : formatPosition(at)}
                </span>
              </div>
              <input
                id={sliderId}
                type="range"
                min={0}
                max={max}
                step={0.1}
                value={Math.min(at, max)}
                aria-valuetext={formatPosition(at)}
                onChange={(e) => onChange({ ...value, atSecs: Number(e.target.value) })}
                className="h-5 w-full cursor-pointer accent-accent"
              />
            </div>
          )
        ) : (
          <div className="flex flex-col gap-2">
            <div className="max-w-56">
              <Select
                label={t("frames.every")}
                value={value.everySecs == null ? ALL : String(value.everySecs)}
                options={[
                  ...INTERVALS.map((s) => ({
                    value: String(s),
                    label: s < 60 ? t("frames.seconds", { n: number(s) }) : t("frames.minutes", { n: number(s / 60) }),
                  })),
                  { value: ALL, label: t("frames.everyFrame") },
                ]}
                onChange={(v) => onChange({ ...value, everySecs: v === ALL ? null : Number(v) })}
              />
            </div>
            <p className="text-[13px] text-label-secondary" aria-live="polite">
              {count != null ? t("frames.count", { n: new Intl.NumberFormat(intlLocale()).format(count) }) : t("frames.folder")}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
