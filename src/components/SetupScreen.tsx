import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { Info, Plus } from "lucide-react";
import { useCallback, useEffect, useRef } from "react";

import type { TargetFormat } from "../bindings/TargetFormat";
import { t } from "../i18n";
import { revealVariants } from "../lib/motion";
import { allWarnings, canStart, commonTargets, readyItems, totalEstimate } from "../state/queue";
import type { QueueController } from "../state/useQueue";
import { AdvancedPanel } from "./AdvancedPanel";
import { DestinationRow } from "./DestinationRow";
import { FileCard } from "./FileCard";
import { FileList } from "./FileList";
import { FormatGrid } from "./FormatGrid";
import { FramesPanel } from "./FramesPanel";
import { LocationRow } from "./LocationRow";
import { PlanFooterInfo, PlanSummary, hasStrongWarning, isRemux } from "./PlanSummary";
import { QualityPicker } from "./QualityPicker";
import { Button } from "./ui/Button";
import { Page } from "./ui/Page";

/** Formats whose result does not depend on the quality preset. */
const FIXED_QUALITY: TargetFormat[] = ["bmp", "tiff", "stillGif", "ico"];

function SectionTitle({ children }: { children: string }) {
  return <h2 className="mb-3 px-1 text-[17px] font-semibold tracking-[-0.01em]">{children}</h2>;
}

/** Files, target format, quality, advanced options, summary and the primary action. */
export function SetupScreen({ q }: { q: QueueController }) {
  const { state } = q;
  const reduceMotion = useReducedMotion();
  const single = state.items.length === 1 && state.items[0].file != null;
  const targets = commonTargets(state.items);
  const ready = readyItems(state);
  const analyzing = state.items.some((i) => i.status === "analyzing");
  const option = targets.find((o) => o.format === state.target) ?? null;
  const warnings = allWarnings(ready);
  const planError = ready.find((i) => i.planError)?.planError ?? null;
  const allPlanned = ready.length > 0 && ready.every((i) => i.plan);
  const estimate = allPlanned ? totalEstimate(ready) : null;
  const remux = allPlanned && ready.every((i) => isRemux(i.plan!));
  // Pictures from videos: the moment must exist in every video.
  const durations = ready.map((i) => i.file?.media.durationSecs ?? null);
  const shortest = durations.every((d) => d != null) && durations.length > 0 ? Math.min(...(durations as number[])) : null;
  const counts = ready.map((i) => i.plan?.frames?.count ?? null);
  const pictures = allPlanned && counts.every((n) => n != null) ? (counts as number[]).reduce((a, b) => a + b, 0) : null;
  const hasLocation = ready.some((i) => i.file?.media.hasLocation);

  // Bring quality and warnings into view once the options have appeared, and
  // whenever an important warning shows up, so it never stays below the fold.
  const summaryRef = useRef<HTMLDivElement>(null);
  const revealed = useRef(state.target != null);
  const scrollToSummary = useCallback(() => {
    summaryRef.current?.scrollIntoView({ block: "nearest", behavior: reduceMotion ? "auto" : "smooth" });
  }, [reduceMotion]);
  const strong = hasStrongWarning(warnings) || q.lowSpace != null;
  useEffect(() => {
    if (!strong || !revealed.current) return;
    const timer = setTimeout(scrollToSummary, 60);
    return () => clearTimeout(timer);
  }, [strong, warnings.length, scrollToSummary]);

  const footer = (
    <>
      <div className="min-w-0 flex-1">
        {state.target && ready.length > 0 && (
          <PlanFooterInfo estimate={estimate} total={ready.length > 1} remux={remux} loading={!allPlanned && !planError} />
        )}
      </div>
      <Button variant="primary" size="lg" disabled={!canStart(state)} onClick={() => void q.start()} className="min-w-44">
        {ready.length > 1 ? t("actions.convertAll", { n: ready.length }) : t("actions.convert")}
      </Button>
    </>
  );

  return (
    <Page footer={footer}>
      <div className="flex flex-col gap-6">
        {single ? (
          <div className="flex flex-col items-end gap-1">
            <div className="w-full">
              <FileCard file={state.items[0].file!} onRemove={q.reset} />
            </div>
            <Button variant="plain" size="sm" icon={<Plus size={15} aria-hidden />} onClick={() => void q.chooseFiles()}>
              {t("files.add")}
            </Button>
          </div>
        ) : (
          <FileList items={state.items} onRemove={q.remove} onAdd={() => void q.chooseFiles()} onClear={q.reset} />
        )}

        <AnimatePresence initial={false}>
          {state.notice === "cancelled" && (
            <motion.p
              variants={revealVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              role="status"
              className="-mt-2 flex items-center gap-2 overflow-hidden px-1 text-[13px] text-label-secondary"
            >
              <Info size={15} aria-hidden />
              {t("cancelled")}
            </motion.p>
          )}
        </AnimatePresence>

        <section>
          <SectionTitle>{t("formats.title")}</SectionTitle>
          {targets.length > 0 ? (
            <>
              {!single && <p className="-mt-2 mb-3 px-1 text-[13px] text-label-secondary">{t("formats.common")}</p>}
              <FormatGrid options={targets} selected={state.target} onSelect={q.selectTarget} />
            </>
          ) : (
            !analyzing && <p className="px-1 text-[14px] text-label-secondary">{t("formats.none")}</p>
          )}
        </section>

        <AnimatePresence initial={false}>
          {option && (
            <motion.div
              variants={revealVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              onAnimationComplete={(definition) => {
                if (definition !== "animate" || revealed.current) return;
                revealed.current = true;
                scrollToSummary();
              }}
              className="flex flex-col gap-6"
            >
              {option.group === "frames" && (
                <section>
                  <SectionTitle>{t("frames.title")}</SectionTitle>
                  <FramesPanel value={state.frames} onChange={q.setFrames} duration={shortest} count={pictures} />
                </section>
              )}
              {!FIXED_QUALITY.includes(option.format) && (
                <section>
                  <SectionTitle>{t("quality.title")}</SectionTitle>
                  <QualityPicker value={state.preset} onChange={q.selectPreset} />
                </section>
              )}
              <AdvancedPanel option={option} value={state.advanced} onChange={q.setAdvanced} />
              {hasLocation && option.group !== "frames" && (
                <LocationRow checked={state.removeLocation} onChange={q.setRemoveLocation} />
              )}
              {/* Margin keeps the content clear of the faded edge above the footer. */}
              <div ref={summaryRef} className="flex scroll-mb-10 flex-col gap-4">
                <PlanSummary warnings={warnings} error={planError} lowSpace={q.lowSpace} />
                <DestinationRow />
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </Page>
  );
}
