import { motion } from "motion/react";
import { useId, useRef, type KeyboardEvent } from "react";

import { spring } from "../../lib/motion";

interface SegmentedProps<T extends string> {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
}

/** macOS-style segmented control: one ARIA radio group, arrow keys move the selection. */
export function Segmented<T extends string>({ label, value, options, onChange }: SegmentedProps<T>) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const indicator = useId();

  const onKeyDown = (e: KeyboardEvent, index: number) => {
    const delta = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (delta === undefined) return;
    e.preventDefault();
    const next = (index + delta + options.length) % options.length;
    onChange(options[next].value);
    refs.current[next]?.focus();
  };

  return (
    <div role="radiogroup" aria-label={label} className="flex rounded-[12px] bg-fill p-[3px]">
      {options.map((option, index) => {
        const active = option.value === value;
        return (
          <button
            key={option.value}
            ref={(el) => {
              refs.current[index] = el;
            }}
            type="button"
            role="radio"
            aria-checked={active}
            tabIndex={active ? 0 : -1}
            onClick={() => onChange(option.value)}
            onKeyDown={(e) => onKeyDown(e, index)}
            className={`relative h-9 flex-1 rounded-[9px] px-2 text-[14px] font-medium transition-colors ${
              active ? "text-label" : "text-label-secondary hover:text-label"
            }`}
          >
            {active && (
              <motion.span
                layoutId={indicator}
                transition={spring}
                className="absolute inset-0 rounded-[9px] bg-surface-raised shadow-[0_1px_3px_rgb(0_0_0/0.12),0_1px_1px_rgb(0_0_0/0.04)]"
              />
            )}
            <span className="relative">{option.label}</span>
          </button>
        );
      })}
    </div>
  );
}
