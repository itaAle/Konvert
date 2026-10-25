import { motion, type HTMLMotionProps } from "motion/react";
import type { ReactNode } from "react";

type Variant = "primary" | "secondary" | "plain";
type Size = "md" | "lg";

interface ButtonProps extends Omit<HTMLMotionProps<"button">, "children"> {
  variant?: Variant;
  size?: Size;
  icon?: ReactNode;
  children: ReactNode;
}

const variants: Record<Variant, string> = {
  primary:
    "bg-accent text-white shadow-[0_1px_2px_rgb(0_0_0/0.12),0_4px_14px_color-mix(in_srgb,var(--k-accent)_35%,transparent)] hover:bg-accent-hover disabled:bg-fill-strong disabled:text-label-tertiary disabled:shadow-none",
  secondary: "bg-fill text-label hover:bg-fill-strong disabled:text-label-tertiary",
  plain: "text-accent hover:bg-accent-soft disabled:text-label-tertiary",
};

const sizes: Record<Size, string> = {
  md: "h-9 px-4 text-[14px] gap-1.5",
  lg: "h-12 px-7 text-[16px] gap-2",
};

/** Pill-shaped button with a subtle press animation. */
export function Button({ variant = "secondary", size = "md", icon, children, className = "", ...rest }: ButtonProps) {
  return (
    <motion.button
      type="button"
      whileTap={rest.disabled ? undefined : { scale: 0.97 }}
      className={`inline-flex shrink-0 items-center justify-center rounded-full font-semibold tracking-[-0.01em] transition-colors duration-150 disabled:cursor-not-allowed ${variants[variant]} ${sizes[size]} ${className}`}
      {...rest}
    >
      {icon}
      {children}
    </motion.button>
  );
}
