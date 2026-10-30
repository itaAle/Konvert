import { useId } from "react";

import type { Volume } from "../bindings/Volume";
import { t } from "../i18n";
import { Segmented } from "./ui/Segmented";

interface VolumePanelProps {
  value: Volume | null;
  onChange: (value: Volume | null) => void;
}

type Mode = "original" | "normalize" | "gain";

/** "+6 dB", "−4,5 dB": a gain as it is read. */
export function formatGain(db: number): string {
  const number = Math.abs(db).toLocaleString(undefined, { maximumFractionDigits: 1 });
  return `${db > 0 ? "+" : db < 0 ? "−" : ""}${number} dB`;
}

/** The loudness of the result: as it is, normalized, or louder or quieter by some decibels. */
export function VolumePanel({ value, onChange }: VolumePanelProps) {
  const sliderId = useId();
  const mode: Mode = value == null ? "original" : value.mode;
  const db = value?.mode === "gain" ? value.db : 0;

  const choose = (next: Mode) => {
    if (next === "original") onChange(null);
    else if (next === "normalize") onChange({ mode: "normalize" });
    else onChange({ mode: "gain", db: db || 3 });
  };

  return (
    <div className="flex flex-col gap-3 rounded-[14px] bg-surface px-4 py-3 shadow-card">
      <Segmented<Mode>
        label={t("volume.heading")}
        value={mode}
        options={[
          { value: "original", label: t("volume.original") },
          { value: "normalize", label: t("volume.normalize") },
          { value: "gain", label: t("volume.gain") },
        ]}
        onChange={choose}
      />
      {mode === "gain" && (
        <div className="flex flex-col gap-1.5">
          <div className="flex items-baseline justify-between gap-3">
            <label htmlFor={sliderId} className="text-[12px] font-medium text-label-secondary">
              {t("volume.amount")}
            </label>
            <span className="text-[13px] text-label tabular-nums" aria-hidden>
              {formatGain(db)}
            </span>
          </div>
          <input
            id={sliderId}
            type="range"
            min={-20}
            max={20}
            step={0.5}
            value={db}
            aria-valuetext={formatGain(db)}
            onChange={(e) => onChange({ mode: "gain", db: Number(e.target.value) })}
            className="h-5 w-full cursor-pointer accent-accent"
          />
        </div>
      )}
      <p className="px-1 text-[12px] leading-snug text-label-secondary">
        {t(mode === "normalize" ? "volume.normalizeHint" : mode === "gain" ? "volume.gainHint" : "volume.originalHint")}
      </p>
    </div>
  );
}
