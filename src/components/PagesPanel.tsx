import { useId } from "react";

import type { PdfOptions } from "../bindings/PdfOptions";
import { errorMessage, intlLocale, t } from "../i18n";
import { Select } from "./ui/Select";

/** Resolutions offered for pictures of pages. */
const DPIS = [72, 150, 300, 600];

interface PagesPanelProps {
  value: PdfOptions;
  onChange: (value: PdfOptions) => void;
  /** Pages of the PDF (the smallest, with several PDFs). */
  pageCount: number;
  /** Pictures that will be created (`null`: not planned yet or invalid pages). */
  count: number | null;
  /** The page range cannot be used. */
  invalid: boolean;
}

function dpiLabel(dpi: number): string {
  if (dpi === 72) return t("pdf.dpiScreen", { n: dpi });
  if (dpi === 300) return t("pdf.dpiPrint", { n: dpi });
  return t("pdf.dpiPlain", { n: dpi });
}

/** PDF to pictures: resolution and which pages. */
export function PagesPanel({ value, onChange, pageCount, count, invalid }: PagesPanelProps) {
  const rangeId = useId();
  const hintId = useId();
  const number = (n: number) => new Intl.NumberFormat(intlLocale()).format(n);

  return (
    <div className="flex flex-col gap-2 px-1">
      <div className="grid grid-cols-2 gap-3">
        <Select
          label={t("pdf.dpi")}
          value={String(value.dpi)}
          options={DPIS.map((dpi) => ({ value: String(dpi), label: dpiLabel(dpi) }))}
          onChange={(v) => onChange({ ...value, dpi: Number(v) })}
        />
        {pageCount > 1 && (
          <div className="flex min-w-0 flex-col gap-1.5">
            <label htmlFor={rangeId} className="px-1 text-[12px] font-medium text-label-secondary">
              {t("pdf.range")}
            </label>
            <input
              id={rangeId}
              type="text"
              inputMode="numeric"
              autoComplete="off"
              spellCheck={false}
              placeholder={t("pdf.rangeAll")}
              value={value.pages ?? ""}
              aria-invalid={invalid}
              aria-describedby={hintId}
              onChange={(e) => onChange({ ...value, pages: e.target.value === "" ? null : e.target.value })}
              className={`h-9 w-full rounded-[10px] bg-fill px-3 text-[14px] text-label transition-colors placeholder:text-label-tertiary hover:bg-fill-strong focus:bg-fill-strong ${
                invalid ? "ring-2 ring-warning" : ""
              }`}
            />
          </div>
        )}
      </div>
      <p id={hintId} className={`text-[13px] ${invalid ? "text-warning" : "text-label-secondary"}`} aria-live="polite">
        {invalid
          ? errorMessage("invalid_pages")
          : pageCount <= 1
          ? `${t("pdf.rangeHintOne")} ${t("pdf.countOne")}`
          : count === 1
            ? `${t("pdf.rangeHint", { n: number(pageCount) })} ${t("pdf.countOne")}`
            : count != null
              ? `${t("pdf.rangeHint", { n: number(pageCount) })} ${t("pdf.count", { n: number(count) })}`
              : t("pdf.rangeHint", { n: number(pageCount) })}
      </p>
    </div>
  );
}
