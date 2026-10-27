import { motion } from "motion/react";
import { ArrowRight, Check, CircleX, Clock } from "lucide-react";

import type { Progress } from "../bindings/Progress";
import { t, type MessageKey } from "../i18n";
import { formatRemaining, formatSpeed } from "../lib/format";
import { spring } from "../lib/motion";
import { counts, overallProgress, type QueueItem } from "../state/queue";
import type { QueueController } from "../state/useQueue";
import { Button } from "./ui/Button";
import { Spinner } from "./ui/Spinner";

const STATUS_LABELS: Partial<Record<QueueItem["status"], MessageKey>> = {
  queued: "status.queued",
  running: "status.running",
  done: "status.done",
  failed: "status.failed",
  cancelled: "status.cancelled",
};

/** "Converting", or "Compressing" for smaller copies. */
const working = (item: QueueItem) => t(item.plan?.compress ? "progress.compressing" : "progress.converting");

/** One-line status of a running/queued item. */
function detailFor(item: QueueItem, cancelling: boolean): string {
  const fraction = item.progress?.fraction ?? null;
  if (cancelling) return t("actions.cancelling");
  if (item.status === "queued") return t("progress.queued");
  if (fraction === null) return t("progress.preparing");
  if (fraction >= 1) return t("progress.finalizing");
  const eta = item.progress?.etaSecs;
  if (eta != null && fraction > 0.02) {
    return eta < 3 ? t("progress.almostDone") : t("progress.remaining", { time: formatRemaining(eta) });
  }
  return working(item);
}

function Bar({ fraction, label, size = "lg" }: { fraction: number | null; label: string; size?: "lg" | "sm" }) {
  const percent = fraction != null ? Math.round(fraction * 100) : undefined;
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent}
      className={`relative w-full overflow-hidden rounded-full bg-fill-strong ${size === "lg" ? "h-2" : "h-1.5"}`}
    >
      {fraction != null ? (
        <motion.div
          className="h-full rounded-full bg-accent"
          initial={false}
          animate={{ width: `${Math.max(2, fraction * 100)}%` }}
          transition={spring}
        />
      ) : (
        <motion.div
          className="absolute inset-y-0 w-1/3 rounded-full bg-accent/70"
          animate={{ left: ["-35%", "100%"] }}
          transition={{ duration: 1.3, repeat: Infinity, ease: "easeInOut" }}
        />
      )}
    </div>
  );
}

function speedText(progress: Progress | null): string | null {
  return progress?.speed != null ? t("progress.speed", { speed: formatSpeed(progress.speed) }) : null;
}

/** Large single-file view (also for several files combined into one PDF). */
function SingleProgress({ q, item, name = item.name }: { q: QueueController; item: QueueItem; name?: string }) {
  const fraction = item.progress?.fraction ?? null;
  const percent = fraction != null ? Math.round(fraction * 100) : null;
  // A smaller copy keeps the format of its file.
  const targetLabel = item.plan?.compress
    ? t("compress.cardFormat", { format: item.name.split(".").pop()?.toUpperCase() ?? "" })
    : (item.file?.targets.find((o) => o.format === q.state.target)?.label ?? "");
  const speed = item.status === "running" && fraction != null && fraction < 1 ? speedText(item.progress) : null;
  const cancelling = q.state.cancelling;

  return (
    <section className="w-full rounded-[28px] bg-surface px-10 py-12 text-center shadow-card-lg">
      <p className="flex items-center justify-center gap-2 text-[14px] text-label-secondary">
        <span className="max-w-[60%] truncate">{name}</span>
        <ArrowRight size={14} aria-hidden className="shrink-0" />
        <span className="font-semibold text-label">{targetLabel}</span>
      </p>
      <p className="mt-6 text-[56px] leading-none font-semibold tracking-[-0.03em] tabular-nums">
        {percent != null ? `${percent}%` : " "}
      </p>
      <div className="mx-auto mt-7 max-w-md">
        <Bar fraction={fraction} label={working(item)} />
      </div>
      <p className="mt-4 text-[14px] text-label-secondary" aria-live="polite">
        {detailFor(item, cancelling)}
        {speed && !cancelling && <span className="text-label-tertiary"> · {speed}</span>}
      </p>
      <Button className="mt-9" onClick={q.cancelAll} disabled={item.jobId === null || cancelling}>
        {cancelling ? t("actions.cancelling") : t("actions.cancel")}
      </Button>
    </section>
  );
}

function RowIcon({ item }: { item: QueueItem }) {
  switch (item.status) {
    case "done":
      return <Check size={16} className="text-success" aria-label={t("status.done")} />;
    case "failed":
      return <CircleX size={16} className="text-danger" aria-label={t("status.failed")} />;
    case "cancelled":
      return <CircleX size={16} className="text-label-tertiary" aria-label={t("status.cancelled")} />;
    case "running":
      return <Spinner size={16} label={t("status.running")} />;
    default:
      return <Clock size={16} className="text-label-tertiary" aria-label={t("status.queued")} />;
  }
}

/** Batch view: overall progress plus one row per file. */
function BatchProgress({ q }: { q: QueueController }) {
  const { state } = q;
  const items = state.items.filter((i) => i.status !== "invalid" && i.status !== "ready");
  const c = counts(state);
  const overall = overallProgress(state);

  return (
    <div className="flex w-full flex-col gap-5">
      <section className="rounded-[24px] bg-surface px-7 py-6 shadow-card">
        <div className="flex items-baseline justify-between gap-4">
          <p className="text-[15px] font-semibold">{t("progress.overall", { done: c.finished, total: c.total })}</p>
          <p className="text-[28px] font-semibold tracking-[-0.02em] tabular-nums">{Math.round(overall * 100)}%</p>
        </div>
        <div className="mt-3">
          <Bar fraction={overall} label={t("progress.converting")} />
        </div>
      </section>

      <ul className="divide-y divide-separator overflow-hidden rounded-[20px] bg-surface shadow-card">
        {items.map((item) => (
          <li key={item.key} className="flex items-center gap-3 px-4 py-3">
            <span className="flex size-6 shrink-0 items-center justify-center">
              <RowIcon item={item} />
            </span>
            <div className="min-w-0 flex-1">
              <p className="truncate text-[14px] font-medium" title={item.name}>
                {item.name}
              </p>
              {item.status === "running" ? (
                <div className="mt-1.5 flex items-center gap-3">
                  <Bar fraction={item.progress?.fraction ?? null} label={item.name} size="sm" />
                  <span className="w-28 shrink-0 truncate text-right text-[12px] text-label-tertiary tabular-nums">
                    {detailFor(item, state.cancelling)}
                  </span>
                </div>
              ) : (
                <p className="text-[12px] text-label-secondary">{t(STATUS_LABELS[item.status] ?? "status.queued")}</p>
              )}
            </div>
          </li>
        ))}
      </ul>

      <div className="flex justify-center">
        <Button onClick={q.cancelAll} disabled={state.cancelling}>
          {state.cancelling ? t("actions.cancelling") : t("actions.cancelAll")}
        </Button>
      </div>
    </div>
  );
}

export function ConvertingScreen({ q }: { q: QueueController }) {
  const active = q.state.items.filter((i) => i.status !== "invalid" && i.status !== "ready");
  if (q.state.combined && active.length > 0) {
    return <SingleProgress q={q} item={active[0]} name={t("progress.combining", { n: active.length })} />;
  }
  return active.length === 1 ? <SingleProgress q={q} item={active[0]} /> : <BatchProgress q={q} />;
}
