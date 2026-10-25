// Shared motion settings: soft springs, never bouncy or slow.
import type { Transition, Variants } from "motion/react";

export const spring: Transition = { type: "spring", stiffness: 420, damping: 34, mass: 0.9 };
export const gentleSpring: Transition = { type: "spring", stiffness: 260, damping: 30 };

/** Screen-to-screen transition. */
export const screenVariants: Variants = {
  initial: { opacity: 0, y: 14, scale: 0.985 },
  animate: { opacity: 1, y: 0, scale: 1, transition: gentleSpring },
  exit: { opacity: 0, y: -10, scale: 0.99, transition: { duration: 0.16, ease: "easeIn" } },
};

/** Content that appears inside a screen (height animates from 0). */
export const revealVariants: Variants = {
  initial: { opacity: 0, height: 0 },
  animate: { opacity: 1, height: "auto", transition: gentleSpring },
  exit: { opacity: 0, height: 0, transition: { duration: 0.15 } },
};
