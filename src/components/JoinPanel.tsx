import { Combine } from "lucide-react";
import { useId } from "react";

import { t } from "../i18n";
import { Switch } from "./ui/Switch";

interface JoinPanelProps {
  value: boolean;
  onChange: (join: boolean) => void;
}

/** Audio or video files joined into one, in the order of the list. */
export function JoinPanel({ value, onChange }: JoinPanelProps) {
  const hintId = useId();
  return (
    <div className="flex items-start gap-3 rounded-[14px] bg-surface px-4 py-3 shadow-card">
      <Combine size={17} className="mt-0.5 shrink-0 text-label-secondary" aria-hidden />
      <div className="min-w-0 flex-1">
        <p className="text-[14px] font-medium">{t("join.title")}</p>
        <p id={hintId} className="mt-0.5 text-[12px] leading-snug text-label-secondary">
          {t("join.hint")}
        </p>
      </div>
      <Switch checked={value} onChange={onChange} label={t("join.title")} describedBy={hintId} />
    </div>
  );
}
