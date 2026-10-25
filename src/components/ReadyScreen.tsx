import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useCallback, useEffect, useRef } from "react";
import { Info } from "lucide-react";

import type { QualityPreset } from "../bindings/QualityPreset";
import type { TargetFormat } from "../bindings/TargetFormat";
import { t } from "../i18n";
import { revealVariants } from "../lib/motion";
import type { Notice, Selection } from "../state/converter";
import { FileCard } from "./FileCard";
import { FormatGrid } from "./FormatGrid";
import { PlanFooterInfo, PlanSummary, hasStrongWarning } from "./PlanSummary";
import { QualityPicker } from "./QualityPicker";
import { Button } from "./ui/Button";
import { Page } from "./ui/Page";

interface ReadyScreenProps {
  sel: Selection;
  notice: Notice | null;
  onSelectTarget: (target: TargetFormat) => void;
  onSelectPreset: (preset: QualityPreset) => void;
  onConvert: () => void;
  onRemove: () => void;
}

function SectionTitle({ children }: { children: string }) {
  return <h2 className="mb-3 px-1 text-[17px] font-semibold tracking-[-0.01em]">{children}</h2>;
}

/** File card, target formats, quality, summary and the single primary action. */
export function ReadyScreen({ sel, notice, onSelectTarget, onSelectPreset, onConvert, onRemove }: ReadyScreenProps) {
  const canConvert = sel.target != null && sel.plan != null;
  const reduceMotion = useReducedMotion();

  // Bring quality and warnings into view once the options have finished
  // appearing, and again whenever an important warning shows up (e.g. GIF
  // drops the audio), so it never stays below the fold.
  const summaryRef = useRef<HTMLDivElement>(null);
  const revealed = useRef(sel.target != null);
  const scrollToSummary = useCallback(() => {
    summaryRef.current?.scrollIntoView({ block: "nearest", behavior: reduceMotion ? "auto" : "smooth" });
  }, [reduceMotion]);
  const strong = hasStrongWarning(sel.plan);
  useEffect(() => {
    if (!strong || !revealed.current) return;
    const timer = setTimeout(scrollToSummary, 60);
    return () => clearTimeout(timer);
  }, [strong, sel.plan, scrollToSummary]);

  const footer = (
    <>
      <div className="min-w-0 flex-1">
        {sel.target && <PlanFooterInfo plan={sel.plan} loading={sel.plan == null && sel.planError == null} />}
      </div>
      <Button variant="primary" size="lg" disabled={!canConvert} onClick={onConvert} className="min-w-44">
        {t("actions.convert")}
      </Button>
    </>
  );

  return (
    <Page footer={footer}>
      <div className="flex flex-col gap-6">
        <FileCard file={sel.file} onRemove={onRemove} />

        <AnimatePresence initial={false}>
          {notice === "cancelled" && (
            <motion.p
              variants={revealVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              role="status"
              className="flex items-center gap-2 overflow-hidden px-1 text-[13px] text-label-secondary"
            >
              <Info size={15} aria-hidden />
              {t("cancelled")}
            </motion.p>
          )}
        </AnimatePresence>

        <section>
          <SectionTitle>{t("formats.title")}</SectionTitle>
          <FormatGrid options={sel.file.targets} selected={sel.target} onSelect={onSelectTarget} />
        </section>

        <AnimatePresence initial={false}>
          {sel.target && (
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
              <section>
                <SectionTitle>{t("quality.title")}</SectionTitle>
                <QualityPicker value={sel.preset} onChange={onSelectPreset} />
              </section>
              {/* Margin keeps the content clear of the faded edge above the footer. */}
              <div ref={summaryRef} className="scroll-mb-10">
                <PlanSummary plan={sel.plan} error={sel.planError} />
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </Page>
  );
}
