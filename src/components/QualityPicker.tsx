import { AnimatePresence, motion } from "motion/react";
import { useRef, type KeyboardEvent } from "react";

import type { QualityPreset } from "../bindings/QualityPreset";
import { t, type MessageKey } from "../i18n";
import { spring } from "../lib/motion";

const PRESETS: { value: QualityPreset; label: MessageKey; hint: MessageKey }[] = [
  { value: "max", label: "quality.max", hint: "quality.maxHint" },
  { value: "balanced", label: "quality.balanced", hint: "quality.balancedHint" },
  { value: "small", label: "quality.small", hint: "quality.smallHint" },
];

interface QualityPickerProps {
  value: QualityPreset;
  onChange: (preset: QualityPreset) => void;
}

/** iOS-style segmented control for the three presets. */
export function QualityPicker({ value, onChange }: QualityPickerProps) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const current = PRESETS.find((p) => p.value === value) ?? PRESETS[1];

  const onKeyDown = (e: KeyboardEvent, index: number) => {
    const delta = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (delta === undefined) return;
    e.preventDefault();
    const next = (index + delta + PRESETS.length) % PRESETS.length;
    onChange(PRESETS[next].value);
    refs.current[next]?.focus();
  };

  return (
    <div>
      <div role="radiogroup" aria-label={t("quality.title")} className="flex rounded-[12px] bg-fill p-[3px]">
        {PRESETS.map((preset, index) => {
          const active = preset.value === value;
          return (
            <button
              key={preset.value}
              ref={(el) => {
                refs.current[index] = el;
              }}
              type="button"
              role="radio"
              aria-checked={active}
              tabIndex={active ? 0 : -1}
              onClick={() => onChange(preset.value)}
              onKeyDown={(e) => onKeyDown(e, index)}
              className={`relative h-9 flex-1 rounded-[9px] text-[14px] font-medium transition-colors ${
                active ? "text-label" : "text-label-secondary hover:text-label"
              }`}
            >
              {active && (
                <motion.span
                  layoutId="preset-indicator"
                  transition={spring}
                  className="absolute inset-0 rounded-[9px] bg-surface-raised shadow-[0_1px_3px_rgb(0_0_0/0.12),0_1px_1px_rgb(0_0_0/0.04)]"
                />
              )}
              <span className="relative">{t(preset.label)}</span>
            </button>
          );
        })}
      </div>
      <div className="relative mt-2 h-5 px-1">
        <AnimatePresence mode="wait" initial={false}>
          <motion.p
            key={current.value}
            initial={{ opacity: 0, y: 3 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -3 }}
            transition={{ duration: 0.15 }}
            className="text-[13px] text-label-secondary"
          >
            {t(current.hint)}
          </motion.p>
        </AnimatePresence>
      </div>
    </div>
  );
}
