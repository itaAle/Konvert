import { motion } from "motion/react";
import { CircleX, ExternalLink, FolderOpen } from "lucide-react";

import { errorMessage, t } from "../i18n";
import { formatBytes } from "../lib/format";
import { counts, type QueueItem } from "../state/queue";
import type { QueueController } from "../state/useQueue";
import { Button } from "./ui/Button";

function Checkmark() {
  return (
    <motion.svg
      width="88"
      height="88"
      viewBox="0 0 88 88"
      aria-hidden
      initial={{ scale: 0.6, opacity: 0 }}
      animate={{ scale: 1, opacity: 1 }}
      transition={{ type: "spring", stiffness: 300, damping: 18 }}
    >
      <circle cx="44" cy="44" r="44" className="fill-success" />
      <motion.path
        d="M26 45 L39 58 L63 32"
        fill="none"
        stroke="white"
        strokeWidth="7"
        strokeLinecap="round"
        strokeLinejoin="round"
        initial={{ pathLength: 0 }}
        animate={{ pathLength: 1 }}
        transition={{ delay: 0.15, duration: 0.35, ease: "easeOut" }}
      />
    </motion.svg>
  );
}

const outputName = (item: QueueItem) => item.outcome?.output.split(/[\\/]/).pop() ?? item.name;

/** Number of files when a series of pictures was saved in a new folder. */
const folderFiles = (item: QueueItem) => ((item.outcome?.files ?? 1) > 1 ? item.outcome!.files : null);

/** Single file (or one folder of pictures) converted successfully. */
export function DoneScreen({ q, item }: { q: QueueController; item: QueueItem }) {
  const files = folderFiles(item);
  return (
    <div className="flex flex-col items-center justify-center text-center">
      <Checkmark />
      <h1 className="mt-6 text-[26px] font-semibold tracking-[-0.02em]">{t("done.title")}</h1>
      <p className="mt-2 max-w-lg truncate text-[15px] text-label-secondary" title={item.outcome?.output}>
        {files != null
          ? t(item.plan?.target === "pdf" ? "done.savedDocuments" : "done.savedFolder", { n: files, name: outputName(item) })
          : t("done.saved", { name: outputName(item) })}{" "}
        · {formatBytes(item.outcome?.sizeBytes ?? 0)}
      </p>
      <div className="mt-9 flex flex-wrap items-center justify-center gap-3">
        <Button
          variant="primary"
          size="lg"
          icon={files != null ? <FolderOpen size={18} aria-hidden /> : <ExternalLink size={18} aria-hidden />}
          onClick={() => item.jobId != null && q.openOutput(item.jobId)}
        >
          {files != null ? t("actions.openFolder") : t("actions.open")}
        </Button>
        <Button
          size="lg"
          icon={<FolderOpen size={18} aria-hidden />}
          onClick={() => item.jobId != null && q.revealOutput(item.jobId)}
        >
          {t("actions.reveal")}
        </Button>
      </div>
      <Button variant="plain" className="mt-5" onClick={q.reset}>
        {t("actions.another")}
      </Button>
    </div>
  );
}

/** Several files: summary and one row per file. */
export function BatchDoneScreen({ q }: { q: QueueController }) {
  const { state } = q;
  const c = counts(state);
  const items = state.items.filter((i) => i.status === "done" || i.status === "failed" || i.status === "cancelled");
  const firstDone = items.find((i) => i.status === "done");
  const title =
    c.failed + c.cancelled === 0
      ? t("done.batchAll", { n: c.done })
      : c.done === 0
        ? t("done.batchNone")
        : t("done.batchPartial", { ok: c.done, failed: c.failed + c.cancelled });

  return (
    <div className="flex w-full flex-col items-center gap-6">
      {c.done > 0 && <Checkmark />}
      <h1 className="text-center text-[24px] font-semibold tracking-[-0.02em]">{title}</h1>
      <ul className="w-full divide-y divide-separator overflow-hidden rounded-[20px] bg-surface shadow-card">
        {items.map((item) => (
          <li key={item.key} className="flex items-center gap-3 py-2.5 pr-2 pl-4">
            <div className="min-w-0 flex-1">
              <p className="truncate text-[14px] font-medium" title={item.outcome?.output ?? item.name}>
                {item.status === "done" ? outputName(item) : item.name}
              </p>
              <p className={`truncate text-[12px] ${item.status === "done" ? "text-label-secondary" : "text-danger"}`}>
                {item.status === "done"
                  ? [folderFiles(item) != null && t("done.pictures", { n: folderFiles(item)! }), formatBytes(item.outcome?.sizeBytes ?? 0)]
                      .filter(Boolean)
                      .join(" · ")
                  : item.status === "cancelled"
                    ? t("status.cancelled")
                    : errorMessage(item.error?.code ?? "unknown")}
              </p>
            </div>
            {item.status === "done" && item.jobId != null ? (
              <>
                <button
                  type="button"
                  onClick={() => q.openOutput(item.jobId!)}
                  aria-label={`${t("actions.open")}: ${outputName(item)}`}
                  title={t("actions.open")}
                  className="flex size-8 items-center justify-center rounded-full text-accent hover:bg-accent-soft"
                >
                  <ExternalLink size={16} aria-hidden />
                </button>
                <button
                  type="button"
                  onClick={() => q.revealOutput(item.jobId!)}
                  aria-label={`${t("actions.reveal")}: ${outputName(item)}`}
                  title={t("actions.reveal")}
                  className="flex size-8 items-center justify-center rounded-full text-accent hover:bg-accent-soft"
                >
                  <FolderOpen size={16} aria-hidden />
                </button>
              </>
            ) : (
              <CircleX size={16} className="mr-2 text-label-tertiary" aria-hidden />
            )}
          </li>
        ))}
      </ul>
      <div className="flex flex-wrap items-center justify-center gap-3">
        {firstDone?.jobId != null && (
          <Button size="lg" icon={<FolderOpen size={18} aria-hidden />} onClick={() => q.revealOutput(firstDone.jobId!)}>
            {t("actions.reveal")}
          </Button>
        )}
        <Button variant="primary" size="lg" onClick={q.reset}>
          {t("actions.others")}
        </Button>
      </div>
    </div>
  );
}
