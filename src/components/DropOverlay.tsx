import { AnimatePresence, motion } from "motion/react";
import { ArrowDownToLine } from "lucide-react";

import { t } from "../i18n";

/** Shown over any screen except the first one while files are dragged over the window. */
export function DropOverlay({ visible }: { visible: boolean }) {
  return (
    <AnimatePresence>
      {visible && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.15 }}
          className="pointer-events-none fixed inset-0 z-50 flex items-center justify-center bg-window/70 p-6 backdrop-blur-md"
        >
          <motion.div
            initial={{ scale: 0.96 }}
            animate={{ scale: 1 }}
            className="flex h-full w-full flex-col items-center justify-center gap-4 rounded-[32px] border-2 border-dashed border-accent bg-accent-soft text-accent"
          >
            <ArrowDownToLine size={40} strokeWidth={1.8} aria-hidden />
            <p className="text-[22px] font-semibold">{t("drop.release")}</p>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
