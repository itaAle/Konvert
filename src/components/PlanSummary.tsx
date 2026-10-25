import { AnimatePresence, motion } from "motion/react";
import { Info, TriangleAlert, Zap } from "lucide-react";

import type { ConversionPlan } from "../bindings/ConversionPlan";
import type { ConversionWarning } from "../bindings/ConversionWarning";
import type { ErrorPayload } from "../bindings/ErrorPayload";
import { errorMessage, t } from "../i18n";
import { formatBytes } from "../lib/format";

/** Warnings shown with the stronger (orange) style. */
const STRONG: ConversionWarning[] = [
  "lossyToLossless",
  "gifLimitations",
  "audioDropped",
  "coverArtDropped",
  "channelsDownmixed",
];

/** The plan has a warning the user really should read. */
export function hasStrongWarning(plan: ConversionPlan | null): boolean {
  return plan?.warnings.some((w) => STRONG.includes(w)) ?? false;
}

/** Every stream is copied as-is: fast and lossless. */
export function isRemux(plan: ConversionPlan): boolean {
  return (
    (plan.video != null || plan.audio != null) &&
    (plan.video?.mode ?? "copy") === "copy" &&
    (plan.audio?.mode ?? "copy") === "copy"
  );
}

/** "circa 4,2 MB", "4,2 MB" or "—". */
export function formatEstimate(plan: ConversionPlan): string {
  const e = plan.estimate;
  if (!e) return "—";
  return e.approximate ? t("summary.approximate", { size: formatBytes(e.bytes) }) : formatBytes(e.bytes);
}

/** Estimate and "no re-encoding" badge, shown next to the primary action. */
export function PlanFooterInfo({ plan, loading }: { plan: ConversionPlan | null; loading: boolean }) {
  if (loading) return <span className="inline-block h-4 w-40 animate-pulse rounded-full bg-fill-strong" />;
  if (!plan) return null;
  return (
    <p className="flex min-w-0 items-center gap-3 text-[14px]" aria-live="polite">
      <span className="truncate text-label-secondary">
        {t("summary.estimate")}{" "}
        <span className="font-semibold text-label tabular-nums">{formatEstimate(plan)}</span>
      </span>
      {isRemux(plan) && (
        <span className="flex shrink-0 items-center gap-1 font-medium text-success" title={t("summary.remux")}>
          <Zap size={14} aria-hidden />
          {t("summary.remuxShort")}
        </span>
      )}
    </p>
  );
}

/** Warnings (or the planning error) for the current selection. */
export function PlanSummary({ plan, error }: { plan: ConversionPlan | null; error: ErrorPayload | null }) {
  if (error) {
    return (
      <p role="alert" className="rounded-[14px] bg-danger-soft px-4 py-3 text-[13px] text-danger">
        {errorMessage(error.code)}
      </p>
    );
  }
  if (!plan || plan.warnings.length === 0) return null;
  return (
    <div className="flex flex-col gap-2">
      <AnimatePresence initial={false}>
        {plan.warnings.map((warning) => {
          const strong = STRONG.includes(warning);
          const Icon = strong ? TriangleAlert : Info;
          return (
            <motion.p
              key={warning}
              initial={{ opacity: 0, y: -4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0 }}
              className={`flex gap-2.5 rounded-[14px] px-4 py-3 text-[13px] leading-snug ${
                strong ? "bg-warning-soft text-label" : "bg-fill text-label-secondary"
              }`}
            >
              <Icon
                size={16}
                className={`mt-px shrink-0 ${strong ? "text-warning" : "text-label-tertiary"}`}
                aria-hidden
              />
              {t(`warnings.${warning}`)}
            </motion.p>
          );
        })}
      </AnimatePresence>
    </div>
  );
}
