import { motion } from "motion/react";
import { ChevronDown } from "lucide-react";
import { useRef, useState, type KeyboardEvent } from "react";

import type { TargetFormat } from "../bindings/TargetFormat";
import type { TargetGroup } from "../bindings/TargetGroup";
import type { TargetOption } from "../bindings/TargetOption";
import { t, type MessageKey } from "../i18n";
import { spring } from "../lib/motion";

interface FormatGridProps {
  options: TargetOption[];
  selected: TargetFormat | null;
  onSelect: (format: TargetFormat) => void;
}

const GROUP_TITLES: Record<TargetGroup, MessageKey> = {
  video: "formats.groupVideo",
  audio: "formats.groupAudio",
  audioOnly: "formats.groupAudioOnly",
  image: "formats.groupImage",
  frames: "formats.groupFrames",
};

/** Notes that the label alone does not tell (translated here, not in the catalog). */
const NOTES: Partial<Record<TargetFormat, MessageKey>> = {
  animatedWebp: "formats.animated",
};

/** Second line of a card: what sets the format apart. */
function noteOf(option: TargetOption): string | null {
  const detail = option.label.match(/\((.+)\)$/)?.[1] ?? null;
  const key = NOTES[option.format];
  if (key) return t(key);
  // "ALAC (M4A)": the container, then "lossless".
  if (option.lossless) return detail ? `${detail} · ${t("formats.lossless")}` : t("formats.lossless");
  if (option.sameAsInput) return t("formats.sameFormat");
  return detail;
}

/**
 * Target formats as selectable cards, grouped (video / audio only / image /
 * frames). Less common formats wait behind "More formats". Behaves as one ARIA
 * radio group: Tab enters it, arrow keys move the selection.
 */
export function FormatGrid({ options, selected, onSelect }: FormatGridProps) {
  const buttons = useRef(new Map<TargetFormat, HTMLButtonElement>());
  const [showAll, setShowAll] = useState(false);
  const hasSecondary = options.some((o) => o.secondary);
  const selectedIsSecondary = options.some((o) => o.format === selected && o.secondary);
  const expanded = showAll || selectedIsSecondary;
  const visible = expanded ? options : options.filter((o) => !o.secondary);

  const groups = (["video", "audio", "audioOnly", "image", "frames"] as TargetGroup[])
    .map((group) => ({ group, items: visible.filter((o) => o.group === group) }))
    .filter((g) => g.items.length > 0);
  const order = groups.flatMap((g) => g.items.map((o) => o.format));
  const focusable = selected ?? order[0];

  const onKeyDown = (e: KeyboardEvent, format: TargetFormat) => {
    const delta = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (delta === undefined) return;
    e.preventDefault();
    const next = order[(order.indexOf(format) + delta + order.length) % order.length];
    onSelect(next);
    buttons.current.get(next)?.focus();
  };

  return (
    <div className="flex flex-col gap-3">
      <div role="radiogroup" aria-label={t("formats.title")} className="flex flex-col gap-4">
        {groups.map(({ group, items }) => (
          <div key={group}>
            {groups.length > 1 && (
              <h3 className="mb-2 px-1 text-[13px] font-medium text-label-secondary">{t(GROUP_TITLES[group])}</h3>
            )}
            <div className="grid grid-cols-4 gap-2.5">
              {items.map((option) => {
                const active = option.format === selected;
                const name = option.label.replace(/ \(.+\)$/, "");
                const note = noteOf(option);
                return (
                  <button
                    key={option.format}
                    ref={(el) => {
                      if (el) buttons.current.set(option.format, el);
                      else buttons.current.delete(option.format);
                    }}
                    type="button"
                    role="radio"
                    aria-checked={active}
                    aria-label={note ? `${name}, ${note}` : name}
                    tabIndex={option.format === focusable ? 0 : -1}
                    onClick={() => onSelect(option.format)}
                    onKeyDown={(e) => onKeyDown(e, option.format)}
                    className={`relative flex h-14 flex-col items-center justify-center rounded-[16px] px-2 transition-colors duration-150 ${
                      active ? "text-white" : "bg-surface text-label shadow-card hover:bg-surface-raised"
                    }`}
                  >
                    {active && (
                      <motion.span
                        layoutId="format-selection"
                        transition={spring}
                        className="absolute inset-0 rounded-[16px] bg-accent shadow-[0_6px_18px_color-mix(in_srgb,var(--k-accent)_40%,transparent)]"
                      />
                    )}
                    <span className="relative text-[15px] font-semibold tracking-[-0.01em]">{name}</span>
                    {note && (
                      <span
                        className={`relative mt-1 max-w-full truncate text-[11px] leading-none ${active ? "text-white/80" : "text-label-tertiary"}`}
                      >
                        {note}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          </div>
        ))}
      </div>
      {hasSecondary && !selectedIsSecondary && (
        <button
          type="button"
          aria-expanded={expanded}
          onClick={() => setShowAll((v) => !v)}
          className="flex items-center gap-1 self-start rounded-full py-1 pr-3 pl-1 text-[13px] font-medium text-accent hover:bg-accent-soft"
        >
          <motion.span animate={{ rotate: expanded ? 180 : 0 }} className="inline-flex">
            <ChevronDown size={15} aria-hidden />
          </motion.span>
          {expanded ? t("formats.fewer") : t("formats.more")}
        </button>
      )}
    </div>
  );
}
