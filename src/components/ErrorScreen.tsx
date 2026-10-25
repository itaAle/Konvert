import { AnimatePresence, motion } from "motion/react";
import { ChevronRight, TriangleAlert } from "lucide-react";
import { useId, useState } from "react";

import type { ErrorPayload } from "../bindings/ErrorPayload";
import { errorMessage, t } from "../i18n";
import { revealVariants } from "../lib/motion";
import { Button } from "./ui/Button";

interface ErrorScreenProps {
  error: ErrorPayload;
  /** Overrides the message derived from the error code. */
  message?: string | null;
  canGoBack: boolean;
  onBack: () => void;
  onChooseAnother: () => void;
}

/** Friendly message; raw engine output only inside "Technical details". */
export function ErrorScreen({ error, message, canGoBack, onBack, onChooseAnother }: ErrorScreenProps) {
  const [open, setOpen] = useState(false);
  const detailsId = useId();

  return (
    <div className="flex flex-col items-center justify-center text-center">
      <div className="flex size-20 items-center justify-center rounded-full bg-danger-soft text-danger">
        <TriangleAlert size={36} strokeWidth={1.8} aria-hidden />
      </div>
      <h1 className="mt-6 text-[24px] font-semibold tracking-[-0.02em]">{t("errors.title")}</h1>
      <p role="alert" className="mt-2 max-w-md text-[15px] leading-relaxed text-label-secondary">
        {message ?? errorMessage(error.code)}
      </p>

      {error.details && (
        <div className="mt-6 w-full max-w-lg text-left">
          <button
            type="button"
            aria-expanded={open}
            aria-controls={detailsId}
            onClick={() => setOpen((v) => !v)}
            className="mx-auto flex items-center gap-1 rounded-full px-3 py-1 text-[13px] font-medium text-label-secondary hover:bg-fill hover:text-label"
          >
            <motion.span animate={{ rotate: open ? 90 : 0 }} className="inline-flex">
              <ChevronRight size={15} aria-hidden />
            </motion.span>
            {t("errors.details")}
          </button>
          <AnimatePresence initial={false}>
            {open && (
              <motion.div
                id={detailsId}
                variants={revealVariants}
                initial="initial"
                animate="animate"
                exit="exit"
                className="overflow-hidden"
              >
                <pre className="mt-3 max-h-48 overflow-auto rounded-[14px] bg-fill p-4 font-mono text-[12px] leading-relaxed whitespace-pre-wrap text-label-secondary">
                  {error.details}
                </pre>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      )}

      <div className="mt-9 flex gap-3">
        {canGoBack && <Button onClick={onBack}>{t("actions.back")}</Button>}
        <Button variant={canGoBack ? "secondary" : "primary"} onClick={onChooseAnother}>
          {t("actions.chooseAnother")}
        </Button>
      </div>
    </div>
  );
}
