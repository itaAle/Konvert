import { ScanText } from "lucide-react";
import { useId } from "react";

import { t } from "../i18n";
import { Switch } from "./ui/Switch";

interface OcrPanelProps {
  value: boolean;
  onChange: (value: boolean) => void;
}

/**
 * Pages without text (scans, photos) are read by OCR on this computer, so
 * the new PDF can be searched and its words copied.
 */
export function OcrPanel({ value, onChange }: OcrPanelProps) {
  const hintId = useId();
  return (
    <div className="flex items-start gap-3 rounded-[14px] bg-surface px-4 py-3 shadow-card">
      <ScanText size={17} className="mt-0.5 shrink-0 text-label-secondary" aria-hidden />
      <div className="min-w-0 flex-1">
        <p className="text-[14px] font-medium">{t("ocr.title")}</p>
        <p id={hintId} className="mt-0.5 text-[12px] leading-snug text-label-secondary">
          {t("ocr.hint")}
        </p>
      </div>
      <Switch checked={value} onChange={onChange} label={t("ocr.title")} describedBy={hintId} />
    </div>
  );
}
