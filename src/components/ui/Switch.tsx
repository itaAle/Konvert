import { motion } from "motion/react";

import { spring } from "../../lib/motion";

interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  describedBy?: string;
}

/** iOS-style toggle (ARIA switch). */
export function Switch({ checked, onChange, label, describedBy }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      aria-describedby={describedBy}
      onClick={() => onChange(!checked)}
      className={`relative h-[26px] w-[44px] shrink-0 rounded-full transition-colors duration-200 ${
        checked ? "bg-success" : "bg-fill-strong"
      }`}
    >
      <motion.span
        className="absolute top-[2px] left-[2px] size-[22px] rounded-full bg-white shadow-[0_2px_4px_rgb(0_0_0/0.2)]"
        animate={{ x: checked ? 18 : 0 }}
        transition={spring}
      />
    </button>
  );
}
