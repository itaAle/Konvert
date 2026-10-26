import { Files } from "lucide-react";
import { useId } from "react";

import type { PageMargin } from "../bindings/PageMargin";
import type { PageOrientation } from "../bindings/PageOrientation";
import type { PageSize } from "../bindings/PageSize";
import type { PdfOptions } from "../bindings/PdfOptions";
import { t } from "../i18n";
import { Segmented } from "./ui/Segmented";
import { Select } from "./ui/Select";
import { Switch } from "./ui/Switch";

interface PdfPanelProps {
  value: PdfOptions;
  onChange: (value: PdfOptions) => void;
  /** Several pictures: one PDF for all of them, or one each. */
  canCombine: boolean;
}

/** Pictures to PDF: paper size, orientation, margins and one PDF or several. */
export function PdfPanel({ value, onChange, canCombine }: PdfPanelProps) {
  const hintId = useId();
  const set = (patch: Partial<PdfOptions>) => onChange({ ...value, ...patch });
  // A page shaped like the photo has no orientation to choose.
  const fit = value.pageSize === "fit";

  return (
    <div className="flex flex-col gap-4">
      <Segmented<PageSize>
        label={t("pdf.size")}
        value={value.pageSize}
        options={[
          { value: "a4", label: t("pdf.a4") },
          { value: "letter", label: t("pdf.letter") },
          { value: "fit", label: t("pdf.fit") },
        ]}
        onChange={(pageSize) => set({ pageSize })}
      />
      <div className="grid grid-cols-2 gap-3 px-1">
        <Select<PageOrientation>
          label={t("pdf.orientation")}
          value={fit ? "auto" : value.orientation}
          disabled={fit}
          options={[
            { value: "auto", label: t("pdf.auto") },
            { value: "portrait", label: t("pdf.portrait") },
            { value: "landscape", label: t("pdf.landscape") },
          ]}
          onChange={(orientation) => set({ orientation })}
        />
        <Select<PageMargin>
          label={t("pdf.margin")}
          value={value.margin}
          options={[
            { value: "none", label: t("pdf.none") },
            { value: "small", label: t("pdf.small") },
            { value: "normal", label: t("pdf.normal") },
          ]}
          onChange={(margin) => set({ margin })}
        />
      </div>
      {canCombine && (
        <div className="flex items-start gap-3 rounded-[14px] bg-surface px-4 py-3 shadow-card">
          <Files size={17} className="mt-0.5 shrink-0 text-label-secondary" aria-hidden />
          <div className="min-w-0 flex-1">
            <p className="text-[14px] font-medium">{t("pdf.combine")}</p>
            <p id={hintId} className="mt-0.5 text-[12px] leading-snug text-label-secondary">
              {t("pdf.combineHint")}
            </p>
          </div>
          <Switch
            checked={value.combine}
            onChange={(combine) => set({ combine })}
            label={t("pdf.combine")}
            describedBy={hintId}
          />
        </div>
      )}
    </div>
  );
}
