import { motion } from "motion/react";
import { ArrowDownToLine, LockKeyhole } from "lucide-react";

import { t } from "../i18n";
import { spring } from "../lib/motion";
import { Button } from "./ui/Button";

interface DropScreenProps {
  dragging: boolean;
  onChoose: () => void;
}

/** First screen: a large drop area (the whole window accepts drops). */
export function DropScreen({ dragging, onChoose }: DropScreenProps) {
  return (
    <div className="flex flex-col items-center justify-center gap-6">
      <motion.section
        aria-label={t("drop.title")}
        animate={{ scale: dragging ? 1.02 : 1 }}
        transition={spring}
        className={`relative flex w-full flex-col items-center rounded-[32px] border-2 border-dashed px-10 py-16 text-center transition-colors duration-200 ${
          dragging
            ? "border-accent bg-accent-soft"
            : "border-separator bg-surface shadow-card"
        }`}
      >
        <motion.div
          animate={{ y: dragging ? 6 : 0 }}
          transition={spring}
          className="mb-6 flex size-20 items-center justify-center rounded-[24px] bg-accent-soft text-accent"
        >
          <ArrowDownToLine size={36} strokeWidth={1.8} aria-hidden />
        </motion.div>
        <h1 className="text-[26px] font-semibold tracking-[-0.02em]">
          {dragging ? t("drop.release") : t("drop.title")}
        </h1>
        <p className="mt-2 max-w-md text-[15px] leading-relaxed text-balance text-label-secondary">
          {t("drop.subtitle")}
        </p>
        <Button variant="primary" size="lg" className="mt-8" onClick={onChoose}>
          {t("drop.choose")}
        </Button>
      </motion.section>
      <p className="flex items-center gap-1.5 text-[13px] text-label-secondary">
        <LockKeyhole size={14} aria-hidden />
        {t("drop.privacy")}
      </p>
    </div>
  );
}
