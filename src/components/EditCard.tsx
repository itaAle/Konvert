import { motion } from "motion/react";
import { SlidersHorizontal } from "lucide-react";

import { t } from "../i18n";
import { spring } from "../lib/motion";

interface EditCardProps {
  selected: boolean;
  onSelect: () => void;
  /** Format shared by every file ("JPG"), if any. */
  format: string | null;
  /** Audio or video files: their tracks are edited. */
  media?: boolean;
}

/**
 * "Edit": an edited copy of every file in its own format (pictures turned,
 * mirrored, cropped, resized; audio and video with chosen tracks). Selected
 * like a format card.
 */
export function EditCard({ selected, onSelect, format, media = false }: EditCardProps) {
  const note = format ? t(media ? "edit.cardMedia" : "edit.cardFormat", { format }) : t("edit.cardHint");
  return (
    <button
      type="button"
      aria-pressed={selected}
      aria-label={`${t("edit.card")}, ${note}`}
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
        <SlidersHorizontal size={18} aria-hidden />
      </span>
      <span className="relative flex min-w-0 flex-col">
        <span className="text-[15px] font-semibold tracking-[-0.01em]">{t("edit.card")}</span>
        <span className={`mt-0.5 truncate text-[12px] ${selected ? "text-white/80" : "text-label-tertiary"}`}>{note}</span>
      </span>
    </button>
  );
}
