import { motion } from "motion/react";
import { Split } from "lucide-react";

import { t } from "../i18n";
import { spring } from "../lib/motion";

interface ExtractCardProps {
  selected: boolean;
  onSelect: () => void;
}

/** "Extract tracks": tracks of the file saved as files of their own. Selected like a format card. */
export function ExtractCard({ selected, onSelect }: ExtractCardProps) {
  const note = t("extract.cardHint");
  return (
    <button
      type="button"
      aria-pressed={selected}
      aria-label={`${t("extract.card")}, ${note}`}
      onClick={onSelect}
      className={`relative flex h-16 w-full items-center gap-3 rounded-[16px] px-4 text-left transition-colors duration-150 ${
        selected ? "text-white" : "bg-surface text-label shadow-card hover:bg-surface-raised"
      }`}
    >
      {selected && (
        <motion.span
          layoutId="format-selection"
          transition={spring}
          className="absolute inset-0 rounded-[16px] bg-accent shadow-[0_6px_18px_color-mix(in_srgb,var(--k-accent)_40%,transparent)]"
        />
      )}
      <span
        className={`relative flex size-9 shrink-0 items-center justify-center rounded-full ${
          selected ? "bg-white/20" : "bg-accent-soft text-accent"
        }`}
      >
        <Split size={18} aria-hidden />
      </span>
      <span className="relative flex min-w-0 flex-col">
        <span className="text-[15px] font-semibold tracking-[-0.01em]">{t("extract.card")}</span>
        <span className={`mt-0.5 truncate text-[12px] ${selected ? "text-white/80" : "text-label-tertiary"}`}>{note}</span>
      </span>
    </button>
  );
}
