import { FileImage, Scale } from "lucide-react";
import { useEffect, useId, useState } from "react";

import type { CompressLevel } from "../bindings/CompressLevel";
import type { CompressOptions } from "../bindings/CompressOptions";
import { intlLocale, t, type MessageKey } from "../i18n";
import { Segmented } from "./ui/Segmented";
import { Switch } from "./ui/Switch";

const LEVELS: { value: CompressLevel; label: MessageKey; hint: MessageKey }[] = [
  { value: "light", label: "compress.light", hint: "compress.lightHint" },
  { value: "strong", label: "compress.strong", hint: "compress.strongHint" },
  { value: "max", label: "compress.max", hint: "compress.maxHint" },
];

/** Upload limits people often meet, in MB. */
const COMMON_LIMITS = [1, 2, 5, 10, 20, 25, 50, 100, 200, 500, 1000, 2000];
const MB = 1_000_000;

/** A limit worth starting from: the largest common one below the file, else half of it. */
export function suggestedLimit(largestBytes: number): number {
  const below = COMMON_LIMITS.filter((mb) => mb * MB < largestBytes);
  if (below.length > 0) return below[below.length - 1] * MB;
  return Math.max(0.1, Math.floor((largestBytes / MB / 2) * 10) / 10) * MB;
}

interface CompressPanelProps {
  value: CompressOptions;
  onChange: (value: CompressOptions) => void;
  /** Size of the largest file, to suggest a limit. */
  largestBytes: number;
  /** Some files are PDFs: their pages may become pictures. */
  documents: boolean;
}

/** Smaller copies: how hard, an optional size limit, PDF pages as pictures. */
export function CompressPanel({ value, onChange, largestBytes, documents }: CompressPanelProps) {
  const limitHintId = useId();
  const limitInputId = useId();
  const pagesHintId = useId();
  const level = LEVELS.find((l) => l.value === value.level) ?? LEVELS[1];
  const limited = value.maxBytes != null;
  const decimal = new Intl.NumberFormat(intlLocale(), { maximumFractionDigits: 1 });
  // The field keeps what is typed (even "2," on the way to "2,5"); the limit follows valid numbers.
  const [text, setText] = useState(limited ? decimal.format(value.maxBytes! / MB) : "");
  useEffect(() => {
    if (value.maxBytes == null) setText("");
  }, [value.maxBytes]);
  const typed = Number(text.replace(",", "."));
  const invalid = limited && (!Number.isFinite(typed) || typed <= 0);

  const setLimit = (on: boolean) => {
    if (!on) {
      onChange({ ...value, maxBytes: null });
      return;
    }
    const bytes = suggestedLimit(largestBytes);
    setText(decimal.format(bytes / MB));
    onChange({ ...value, maxBytes: bytes });
  };

  return (
    <div className="flex flex-col gap-4">
      <div>
        <Segmented<CompressLevel>
          label={t("compress.level")}
          value={value.level}
          options={LEVELS.map((l) => ({ value: l.value, label: t(l.label) }))}
          onChange={(next) => onChange({ ...value, level: next })}
        />
        <p className="mt-2 px-1 text-[13px] text-label-secondary" aria-live="polite">
          {t(level.hint)}
        </p>
      </div>

      <div className="rounded-[14px] bg-surface px-4 py-3 shadow-card">
        <div className="flex items-start gap-3">
          <Scale size={17} className="mt-0.5 shrink-0 text-label-secondary" aria-hidden />
          <div className="min-w-0 flex-1">
            <p className="text-[14px] font-medium">{t("compress.limit")}</p>
            <p id={limitHintId} className="mt-0.5 text-[12px] leading-snug text-label-secondary">
              {t("compress.limitHint")}
            </p>
          </div>
          <Switch checked={limited} onChange={setLimit} label={t("compress.limit")} describedBy={limitHintId} />
        </div>
        {limited && (
          <div className="mt-3 flex items-center gap-2 pl-[29px]">
            <label htmlFor={limitInputId} className="sr-only">
              {t("compress.limitValue")}
            </label>
            <input
              id={limitInputId}
              type="text"
              inputMode="decimal"
              autoComplete="off"
              spellCheck={false}
              value={text}
              aria-invalid={invalid}
              onChange={(e) => {
                setText(e.target.value);
                const mb = Number(e.target.value.replace(",", "."));
                if (Number.isFinite(mb) && mb > 0) onChange({ ...value, maxBytes: Math.round(mb * MB) });
              }}
              className={`h-9 w-28 rounded-[10px] bg-fill px-3 text-right text-[14px] text-label tabular-nums transition-colors hover:bg-fill-strong focus:bg-fill-strong ${
                invalid ? "ring-2 ring-warning" : ""
              }`}
            />
            <span className="text-[14px] text-label-secondary">MB</span>
          </div>
        )}
      </div>

      {documents && (
        <div className="flex items-start gap-3 rounded-[14px] bg-surface px-4 py-3 shadow-card">
          <FileImage size={17} className="mt-0.5 shrink-0 text-label-secondary" aria-hidden />
          <div className="min-w-0 flex-1">
            <p className="text-[14px] font-medium">{t("compress.pages")}</p>
            <p id={pagesHintId} className="mt-0.5 text-[12px] leading-snug text-label-secondary">
              {t("compress.pagesHint")}
            </p>
          </div>
          <Switch
            checked={value.pagesAsPictures}
            onChange={(pagesAsPictures) => onChange({ ...value, pagesAsPictures })}
            label={t("compress.pages")}
            describedBy={pagesHintId}
          />
        </div>
      )}
    </div>
  );
}
