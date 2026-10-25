import { AnimatePresence, motion } from "motion/react";
import { Clapperboard, Music2, Plus, TriangleAlert, X } from "lucide-react";

import { errorMessage, t } from "../i18n";
import { formatBytes, formatDuration } from "../lib/format";
import { NO_TARGETS, type QueueItem } from "../state/queue";
import { formatEstimate } from "./PlanSummary";
import { Button } from "./ui/Button";
import { Spinner } from "./ui/Spinner";

interface FileListProps {
  items: QueueItem[];
  onRemove?: (key: number) => void;
  onAdd?: () => void;
  onClear?: () => void;
}

export function itemProblem(item: QueueItem): string | null {
  if (item.status !== "invalid" || !item.error) return null;
  return item.error.code === NO_TARGETS ? t("files.noTargets") : errorMessage(item.error.code);
}

function ItemIcon({ item }: { item: QueueItem }) {
  if (item.status === "analyzing") {
    return (
      <span className="flex size-9 shrink-0 items-center justify-center rounded-[10px] bg-fill">
        <Spinner size={18} label={t("files.analyzing")} />
      </span>
    );
  }
  if (item.status === "invalid") {
    return (
      <span className="flex size-9 shrink-0 items-center justify-center rounded-[10px] bg-warning-soft text-warning">
        <TriangleAlert size={17} aria-hidden />
      </span>
    );
  }
  const video = item.file?.media.kind !== "audio";
  const Icon = video ? Clapperboard : Music2;
  return (
    <span
      className={`flex size-9 shrink-0 items-center justify-center rounded-[10px] text-white ${
        video ? "bg-gradient-to-b from-[#bf5af2] to-[#8e44d6]" : "bg-gradient-to-b from-[#ff6482] to-[#e8364f]"
      }`}
    >
      <Icon size={17} strokeWidth={2} aria-hidden />
    </span>
  );
}

/** Compact list of the files to convert (two or more). */
export function FileList({ items, onRemove, onAdd, onClear }: FileListProps) {
  return (
    <section aria-label={t("files.title")}>
      <div className="mb-3 flex items-center gap-2 px-1">
        <h2 className="text-[17px] font-semibold tracking-[-0.01em]">{t("files.title")}</h2>
        <span className="text-[14px] text-label-tertiary">{t("files.count", { n: items.length })}</span>
        <span className="flex-1" />
        {onClear && (
          <Button variant="plain" size="sm" onClick={onClear}>
            {t("files.clear")}
          </Button>
        )}
        {onAdd && (
          <Button variant="plain" size="sm" icon={<Plus size={15} aria-hidden />} onClick={onAdd}>
            {t("files.add")}
          </Button>
        )}
      </div>
      <ul className="max-h-[248px] divide-y divide-separator overflow-y-auto rounded-[20px] bg-surface shadow-card">
        <AnimatePresence initial={false}>
          {items.map((item) => {
            const problem = itemProblem(item);
            const media = item.file?.media;
            const facts = media
              ? [
                  media.durationSecs != null ? formatDuration(media.durationSecs) : null,
                  media.sizeBytes != null ? formatBytes(media.sizeBytes) : null,
                ]
                  .filter(Boolean)
                  .join(" · ")
              : "";
            return (
              <motion.li
                key={item.key}
                layout="position"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="flex items-center gap-3 py-2.5 pr-2 pl-3"
              >
                <ItemIcon item={item} />
                <div className="min-w-0 flex-1">
                  <p className="truncate text-[14px] font-medium" title={item.name}>
                    {item.name}
                  </p>
                  <p className={`truncate text-[12px] ${problem ? "text-warning" : "text-label-secondary"}`}>
                    {item.status === "analyzing" ? t("files.analyzing") : (problem ?? facts)}
                  </p>
                </div>
                {item.plan && (
                  <span className="shrink-0 text-[12px] text-label-tertiary tabular-nums">
                    {formatEstimate(item.plan)}
                  </span>
                )}
                {problem && <span className="shrink-0 text-[12px] text-label-tertiary">{t("files.excluded")}</span>}
                {onRemove && (
                  <button
                    type="button"
                    onClick={() => onRemove(item.key)}
                    aria-label={t("files.remove", { name: item.name })}
                    title={t("files.remove", { name: item.name })}
                    className="flex size-7 shrink-0 items-center justify-center rounded-full text-label-tertiary transition-colors hover:bg-fill hover:text-label"
                  >
                    <X size={15} aria-hidden />
                  </button>
                )}
              </motion.li>
            );
          })}
        </AnimatePresence>
      </ul>
    </section>
  );
}
