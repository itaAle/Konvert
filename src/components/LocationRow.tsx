import { MapPin } from "lucide-react";
import { useId } from "react";

import { t } from "../i18n";
import { Switch } from "./ui/Switch";

interface LocationRowProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
}

/** Shown when a file records where it was made: the location can be left out. */
export function LocationRow({ checked, onChange }: LocationRowProps) {
  const hintId = useId();
  return (
    <div className="flex items-start gap-3 rounded-[14px] bg-surface px-4 py-3 shadow-card">
      <MapPin size={17} className="mt-0.5 shrink-0 text-label-secondary" aria-hidden />
      <div className="min-w-0 flex-1">
        <p className="text-[14px] font-medium">{t("location.remove")}</p>
        <p id={hintId} className="mt-0.5 text-[12px] leading-snug text-label-secondary">
          {t("location.hint")}
        </p>
      </div>
      <Switch checked={checked} onChange={onChange} label={t("location.remove")} describedBy={hintId} />
    </div>
  );
}
