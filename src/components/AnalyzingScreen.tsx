import { t } from "../i18n";
import { Spinner } from "./ui/Spinner";

export function AnalyzingScreen({ path }: { path: string }) {
  const name = path.split(/[\\/]/).pop() ?? path;
  return (
    <div className="flex flex-col items-center justify-center gap-4 text-center">
      <Spinner size={32} label={t("analyzing")} />
      <div>
        <p className="text-[17px] font-semibold">{t("analyzing")}</p>
        <p className="mt-1 max-w-md truncate text-[14px] text-label-secondary">{name}</p>
      </div>
    </div>
  );
}
