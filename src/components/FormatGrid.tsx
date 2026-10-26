import { motion } from "motion/react";
import { useRef, type KeyboardEvent } from "react";

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

/**
 * Target formats as selectable cards, grouped (video / audio only / image).
 * Behaves as one ARIA radio group: Tab enters it, arrow keys move the selection.
 */
export function FormatGrid({ options, selected, onSelect }: FormatGridProps) {
  const buttons = useRef(new Map<TargetFormat, HTMLButtonElement>());
  const groups = (["video", "audio", "audioOnly", "image", "frames"] as TargetGroup[])
    .map((group) => ({ group, items: options.filter((o) => o.group === group) }))
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
              const note = option.lossless
                ? t("formats.lossless")
                : option.sameAsInput
                  ? t("formats.sameFormat")
                  : (option.label.match(/\((.+)\)$/)?.[1] ?? null);
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
                  <span className="relative text-[15px] font-semibold tracking-[-0.01em]">
                    {name}
                  </span>
                  {note && (
                    <span
                      className={`relative mt-1 text-[11px] leading-none ${active ? "text-white/80" : "text-label-tertiary"}`}
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
  );
}
