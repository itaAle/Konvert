import { AnimatePresence, motion } from "motion/react";
import { ChevronDown, ChevronUp, LockKeyhole, Plus, TriangleAlert, X } from "lucide-react";

import { errorMessage, t } from "../i18n";
import { formatBytes, formatDuration, formatPages } from "../lib/format";
import { kindStyle } from "../lib/kind";
import { NO_TARGETS, type QueueItem } from "../state/queue";
import { formatEstimate } from "./PlanSummary";
import { Button } from "./ui/Button";
import { Spinner } from "./ui/Spinner";
import { UnlockForm, type UnlockAttempt } from "./Unlock";

interface FileListProps {
  items: QueueItem[];
  onRemove?: (key: number) => void;
  onAdd?: () => void;
  onClear?: () => void;
  /** Files combined into one PDF: their order is the order of the pages. */
  onMove?: (key: number, delta: -1 | 1) => void;
  /** A PDF that needs a password is opened with it. */
  onUnlock?: (key: number, password: string) => ReturnType<UnlockAttempt>;
}

const locked = (item: QueueItem) => item.status === "invalid" && item.error?.code === "pdf_encrypted";

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
  if (locked(item)) {
    return (
      <span className="flex size-9 shrink-0 items-center justify-center rounded-[10px] bg-accent-soft text-accent">
        <LockKeyhole size={17} aria-hidden />
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
  const { Icon, gradient } = kindStyle(item.file?.media.kind);
  return (
    <span
      className={`flex size-9 shrink-0 items-center justify-center rounded-[10px] bg-gradient-to-b text-white ${gradient}`}
    >
      <Icon size={17} strokeWidth={2} aria-hidden />
    </span>
  );
}

/** Compact list of the files to convert (two or more). */
export function FileList({ items, onRemove, onAdd, onClear, onMove, onUnlock }: FileListProps) {
  const title = onMove ? t("files.titleCombine") : t("files.title");
  const moveButton = "flex size-7 shrink-0 items-center justify-center rounded-full text-label-tertiary transition-colors enabled:hover:bg-fill enabled:hover:text-label disabled:opacity-30";
  return (
    <section aria-label={title}>
      <div className="mb-3 flex items-center gap-2 px-1">
        <h2 className="text-[17px] font-semibold tracking-[-0.01em]">{title}</h2>
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
      {onMove && <p className="-mt-2 mb-3 px-1 text-[13px] text-label-secondary">{t("files.orderHint")}</p>}
      <ul className="max-h-[248px] divide-y divide-separator overflow-y-auto rounded-[20px] bg-surface shadow-card">
        <AnimatePresence initial={false}>
          {items.map((item, index) => {
            const needsPassword = locked(item) && onUnlock != null;
            const problem = needsPassword ? null : itemProblem(item);
            const media = item.file?.media;
            const facts = media
              ? [
                  media.durationSecs != null ? formatDuration(media.durationSecs) : null,
                  media.document ? formatPages(media.document.pages) : null,
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
                  {needsPassword ? (
                    <div className="mt-1">
                      <UnlockForm name={item.name} onUnlock={(password) => onUnlock(item.key, password)} compact />
                    </div>
                  ) : (
                    <p className={`truncate text-[12px] ${problem ? "text-warning" : "text-label-secondary"}`}>
                      {item.status === "analyzing" ? t("files.analyzing") : (problem ?? facts)}
                    </p>
                  )}
                </div>
                {item.plan && (
                  <span className="shrink-0 text-[12px] text-label-tertiary tabular-nums">
                    {formatEstimate(item.plan)}
                  </span>
                )}
                {problem && <span className="shrink-0 text-[12px] text-label-tertiary">{t("files.excluded")}</span>}
                {onMove && (
                  <>
                    <button
                      type="button"
                      disabled={index === 0}
                      onClick={() => onMove(item.key, -1)}
                      aria-label={t("files.moveUp", { name: item.name })}
                      title={t("files.moveUp", { name: item.name })}
                      className={moveButton}
                    >
                      <ChevronUp size={16} aria-hidden />
                    </button>
                    <button
                      type="button"
                      disabled={index === items.length - 1}
                      onClick={() => onMove(item.key, 1)}
                      aria-label={t("files.moveDown", { name: item.name })}
                      title={t("files.moveDown", { name: item.name })}
                      className={moveButton}
                    >
                      <ChevronDown size={16} aria-hidden />
                    </button>
                  </>
                )}
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
