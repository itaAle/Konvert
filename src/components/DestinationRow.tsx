import { FolderOpen, X } from "lucide-react";

import { t } from "../i18n";
import { backend } from "../lib/api";
import { usePreferences } from "../state/preferences";
import { Button } from "./ui/Button";

/** `D:\Musica\Convertiti` → `…\Musica\Convertiti` when long. */
export function shortPath(path: string, max = 42): string {
  if (path.length <= max) return path;
  const parts = path.split(/[\\/]/);
  const sep = path.includes("\\") ? "\\" : "/";
  let tail = parts.pop() ?? path;
  while (parts.length > 0 && `…${sep}${parts[parts.length - 1]}${sep}${tail}`.length <= max) {
    tail = `${parts.pop()}${sep}${tail}`;
  }
  return `…${sep}${tail}`;
}

/** Where converted files go; changing it updates the preference. */
export function DestinationRow({ compact = false }: { compact?: boolean }) {
  const { prefs, update } = usePreferences();
  const dir = prefs.outputDir;

  const choose = async () => {
    const picked = await backend.pickFolder(t("destination.pickerTitle")).catch(() => null);
    if (picked) update({ outputDir: picked });
  };

  return (
    <div className={`flex min-w-0 items-center gap-2 text-[13px] ${compact ? "" : "px-1"}`}>
      <FolderOpen size={15} className="shrink-0 text-label-tertiary" aria-hidden />
      {!compact && <span className="shrink-0 text-label-secondary">{t("destination.label")}</span>}
      <span className="min-w-0 truncate font-medium" title={dir ?? undefined}>
        {dir ? shortPath(dir) : t("destination.nextToOriginal")}
      </span>
      <Button variant="plain" size="sm" onClick={choose}>
        {t("destination.change")}
      </Button>
      {dir && (
        <button
          type="button"
          onClick={() => update({ outputDir: null })}
          aria-label={t("destination.nextToOriginal")}
          title={t("destination.nextToOriginal")}
          className="flex size-6 shrink-0 items-center justify-center rounded-full text-label-tertiary hover:bg-fill hover:text-label"
        >
          <X size={13} aria-hidden />
        </button>
      )}
    </div>
  );
}
