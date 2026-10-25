import { motion } from "motion/react";
import { ArrowRight } from "lucide-react";

import { t } from "../i18n";
import { formatRemaining, formatSpeed } from "../lib/format";
import { spring } from "../lib/motion";
import type { ConverterState } from "../state/converter";
import { Button } from "./ui/Button";

type Converting = Extract<ConverterState, { phase: "converting" }>;

export function ConvertingScreen({ state, onCancel }: { state: Converting; onCancel: () => void }) {
  const { sel, progress, status } = state;
  const fraction = progress?.fraction ?? null;
  const percent = fraction != null ? Math.round(fraction * 100) : null;
  const targetLabel = sel.file.targets.find((o) => o.format === sel.target)?.label ?? "";

  let detail: string;
  if (status === "cancelling") detail = t("actions.cancelling");
  else if (status === "starting" || status === "queued") detail = t("progress.queued");
  else if (fraction === null) detail = t("progress.preparing");
  else if (fraction >= 1) detail = t("progress.finalizing");
  else if (progress?.etaSecs != null && fraction > 0.02) {
    detail = progress.etaSecs < 3 ? t("progress.almostDone") : t("progress.remaining", { time: formatRemaining(progress.etaSecs) });
  } else detail = t("progress.converting");

  return (
    <div className="flex flex-col items-center justify-center">
      <section className="w-full rounded-[28px] bg-surface px-10 py-12 text-center shadow-card-lg">
        <p className="flex items-center justify-center gap-2 text-[14px] text-label-secondary">
          <span className="max-w-[60%] truncate">{sel.file.name}</span>
          <ArrowRight size={14} aria-hidden className="shrink-0" />
          <span className="font-semibold text-label">{targetLabel}</span>
        </p>

        <p className="mt-6 text-[56px] leading-none font-semibold tracking-[-0.03em] tabular-nums">
          {percent != null ? `${percent}%` : " "}
        </p>

        <div
          role="progressbar"
          aria-label={t("progress.converting")}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent ?? undefined}
          className="relative mx-auto mt-7 h-2 w-full max-w-md overflow-hidden rounded-full bg-fill-strong"
        >
          {fraction != null ? (
            <motion.div
              className="h-full rounded-full bg-accent"
              initial={{ width: 0 }}
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

        <p className="mt-4 text-[14px] text-label-secondary" aria-live="polite">
          {detail}
          {status === "running" && progress?.speed != null && fraction != null && fraction < 1 && (
            <span className="text-label-tertiary"> · {t("progress.speed", { speed: formatSpeed(progress.speed) })}</span>
          )}
        </p>

        <Button className="mt-9" onClick={onCancel} disabled={state.jobId === null || status === "cancelling"}>
          {status === "cancelling" ? t("actions.cancelling") : t("actions.cancel")}
        </Button>
      </section>
    </div>
  );
}
