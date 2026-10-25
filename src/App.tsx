import { AnimatePresence, MotionConfig, motion } from "motion/react";
import { useEffect, type ReactNode } from "react";

import { AnalyzingScreen } from "./components/AnalyzingScreen";
import { ConvertingScreen } from "./components/ConvertingScreen";
import { DoneScreen } from "./components/DoneScreen";
import { DropOverlay } from "./components/DropOverlay";
import { DropScreen } from "./components/DropScreen";
import { ErrorScreen } from "./components/ErrorScreen";
import { ReadyScreen } from "./components/ReadyScreen";
import { Page } from "./components/ui/Page";
import { backend } from "./lib/api";
import { screenVariants } from "./lib/motion";
import { useConverter } from "./state/useConverter";

export default function App() {
  const c = useConverter();
  const { state } = c;
  const { chooseFiles } = c;

  // Translucent surfaces only when the Mica backdrop is active; then show the
  // window (it starts hidden to avoid a blank flash).
  useEffect(() => {
    backend
      .platformInfo()
      .then((info) => {
        document.documentElement.dataset.vibrancy = String(info.vibrancy);
      })
      .catch(() => {})
      .finally(() => void backend.appReady().catch(() => {}));
  }, []);

  // Ctrl/⌘+O opens the file picker.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "o") {
        e.preventDefault();
        void chooseFiles();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [chooseFiles]);

  let screen: ReactNode;
  switch (state.phase) {
    case "empty":
      screen = <DropScreen dragging={c.dragging} onChoose={chooseFiles} />;
      break;
    case "analyzing":
      screen = <AnalyzingScreen path={state.path} />;
      break;
    case "ready":
      screen = (
        <ReadyScreen
          sel={state.sel}
          notice={state.notice}
          onSelectTarget={c.selectTarget}
          onSelectPreset={c.selectPreset}
          onConvert={c.start}
          onRemove={c.reset}
        />
      );
      break;
    case "converting":
      screen = <ConvertingScreen state={state} onCancel={c.cancel} />;
      break;
    case "done":
      screen = (
        <DoneScreen
          state={state}
          onOpen={() => c.openOutput(state.jobId)}
          onReveal={() => c.revealOutput(state.jobId)}
          onAnother={c.reset}
        />
      );
      break;
    case "error":
      screen = (
        <ErrorScreen
          error={state.error}
          canGoBack={state.sel != null}
          onBack={c.back}
          onChooseAnother={() => {
            c.reset();
            void chooseFiles();
          }}
        />
      );
      break;
  }

  return (
    <MotionConfig reducedMotion="user">
      <main className="flex h-full flex-col">
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={state.phase}
            variants={screenVariants}
            initial="initial"
            animate="animate"
            exit="exit"
            className="flex min-h-0 flex-1 flex-col"
          >
            {state.phase === "ready" ? screen : <Page center>{screen}</Page>}
          </motion.div>
        </AnimatePresence>
      </main>
      <DropOverlay visible={c.dragging && state.phase !== "empty"} />
    </MotionConfig>
  );
}
