import { motion } from "motion/react";
import { ExternalLink, FolderOpen } from "lucide-react";

import { t } from "../i18n";
import { formatBytes } from "../lib/format";
import type { ConverterState } from "../state/converter";
import { Button } from "./ui/Button";

type Done = Extract<ConverterState, { phase: "done" }>;

interface DoneScreenProps {
  state: Done;
  onOpen: () => void;
  onReveal: () => void;
  onAnother: () => void;
}

function Checkmark() {
  return (
    <motion.svg
      width="88"
      height="88"
      viewBox="0 0 88 88"
      aria-hidden
      initial={{ scale: 0.6, opacity: 0 }}
      animate={{ scale: 1, opacity: 1 }}
      transition={{ type: "spring", stiffness: 300, damping: 18 }}
    >
      <circle cx="44" cy="44" r="44" className="fill-success" />
      <motion.path
        d="M26 45 L39 58 L63 32"
        fill="none"
        stroke="white"
        strokeWidth="7"
        strokeLinecap="round"
        strokeLinejoin="round"
        initial={{ pathLength: 0 }}
        animate={{ pathLength: 1 }}
        transition={{ delay: 0.15, duration: 0.35, ease: "easeOut" }}
      />
    </motion.svg>
  );
}

export function DoneScreen({ state, onOpen, onReveal, onAnother }: DoneScreenProps) {
  const name = state.outcome.output.split(/[\\/]/).pop() ?? state.outcome.output;
  return (
    <div className="flex flex-col items-center justify-center text-center">
      <Checkmark />
      <h1 className="mt-6 text-[26px] font-semibold tracking-[-0.02em]">{t("done.title")}</h1>
      <p className="mt-2 max-w-lg truncate text-[15px] text-label-secondary" title={state.outcome.output}>
        {t("done.saved", { name })} · {formatBytes(state.outcome.sizeBytes)}
      </p>
      <div className="mt-9 flex flex-wrap items-center justify-center gap-3">
        <Button variant="primary" size="lg" icon={<ExternalLink size={18} aria-hidden />} onClick={onOpen}>
          {t("actions.open")}
        </Button>
        <Button size="lg" icon={<FolderOpen size={18} aria-hidden />} onClick={onReveal}>
          {t("actions.reveal")}
        </Button>
      </div>
      <Button variant="plain" className="mt-5" onClick={onAnother}>
        {t("actions.another")}
      </Button>
    </div>
  );
}
