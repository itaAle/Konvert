import { AnimatePresence, motion } from "motion/react";
import { useEffect, useId, useRef, type ReactNode } from "react";

import { spring } from "../../lib/motion";

interface ModalProps {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** `sheet`: large settings panel; `alert`: small confirmation. */
  size?: "sheet" | "alert";
}

/** Accessible modal: focus moves inside, Tab stays inside, Esc closes. */
export function Modal({ open, title, onClose, children, size = "sheet" }: ModalProps) {
  const panel = useRef<HTMLDivElement>(null);
  const titleId = useId();
  // Read through a ref: a new `onClose` on every parent render must not re-run
  // the effect below (that would move the focus back to the first control).
  const close = useRef(onClose);
  close.current = onClose;

  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    const focusables = () =>
      [...(panel.current?.querySelectorAll<HTMLElement>(
        'button, [href], select, input, [tabindex]:not([tabindex="-1"])',
      ) ?? [])].filter((el) => !el.hasAttribute("disabled"));
    // Focus the first control once the panel is mounted.
    const timer = setTimeout(() => focusables()[0]?.focus(), 30);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        close.current();
      } else if (e.key === "Tab") {
        const items = focusables();
        if (items.length === 0) return;
        const first = items[0];
        const last = items[items.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      clearTimeout(timer);
      window.removeEventListener("keydown", onKey);
      previous?.focus?.();
    };
  }, [open]);

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="fixed inset-0 z-40 flex items-center justify-center bg-black/25 p-6 backdrop-blur-[2px]"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.15 }}
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) close.current();
          }}
        >
          <motion.div
            ref={panel}
            role="dialog"
            aria-modal="true"
            aria-labelledby={titleId}
            initial={{ opacity: 0, scale: 0.96, y: 10 }}
            animate={{ opacity: 1, scale: 1, y: 0, transition: spring }}
            exit={{ opacity: 0, scale: 0.98, transition: { duration: 0.12 } }}
            className={`flex max-h-full w-full flex-col overflow-hidden rounded-[24px] bg-sheet shadow-card-lg backdrop-blur-2xl ${
              size === "sheet" ? "max-w-[560px]" : "max-w-[400px]"
            }`}
          >
            <h2 id={titleId} className={size === "sheet" ? "sr-only" : "px-6 pt-6 text-[17px] font-semibold"}>
              {title}
            </h2>
            {children}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
