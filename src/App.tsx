import { AnimatePresence, MotionConfig, motion } from "motion/react";
import { Settings } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";

import { AnalyzingScreen } from "./components/AnalyzingScreen";
import { ConvertingScreen } from "./components/ConvertingScreen";
import { BatchDoneScreen, DoneScreen } from "./components/DoneScreen";
import { DropOverlay } from "./components/DropOverlay";
import { DropScreen } from "./components/DropScreen";
import { ErrorScreen } from "./components/ErrorScreen";
import { itemProblem } from "./components/FileList";
import { SettingsSheet } from "./components/SettingsSheet";
import { SetupScreen } from "./components/SetupScreen";
import { Button } from "./components/ui/Button";
import { Modal } from "./components/ui/Modal";
import { Page } from "./components/ui/Page";
import { t } from "./i18n";
import { backend } from "./lib/api";
import { screenVariants } from "./lib/motion";
import { PreferencesProvider, usePreferences } from "./state/preferences";
import { useQueue } from "./state/useQueue";

export default function App() {
  return (
    <PreferencesProvider>
      <MotionConfig reducedMotion="user">
        <Main />
      </MotionConfig>
    </PreferencesProvider>
  );
}

function Main() {
  const { loaded } = usePreferences();
  const q = useQueue();
  const { state, chooseFiles } = q;
  const [settingsOpen, setSettingsOpen] = useState(false);

  // Once the preferences (theme, language) are applied: translucent surfaces if
  // Mica is active, then show the window (it starts hidden to avoid a flash).
  useEffect(() => {
    if (!loaded) return;
    backend
      .platformInfo()
      .then((info) => {
        document.documentElement.dataset.vibrancy = String(info.vibrancy);
      })
      .catch(() => {})
      .finally(() => void backend.appReady().catch(() => {}));
  }, [loaded]);

  // Ctrl/⌘+O: choose files. Ctrl/⌘+,: settings.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      if (e.key.toLowerCase() === "o") {
        e.preventDefault();
        void chooseFiles();
      } else if (e.key === ",") {
        e.preventDefault();
        setSettingsOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [chooseFiles]);

  const only = state.items.length === 1 ? state.items[0] : null;
  let key: string = state.phase;
  let screen: ReactNode;
  switch (state.phase) {
    case "empty":
      screen = <DropScreen dragging={q.dragging} onChoose={chooseFiles} />;
      break;
    case "setup":
      if (only?.status === "analyzing") {
        key = "analyzing";
        screen = <AnalyzingScreen path={only.path} />;
      } else if (only?.status === "invalid") {
        key = "invalid";
        screen = (
          <ErrorScreen
            error={only.error ?? { code: "unknown", details: null }}
            message={itemProblem(only)}
            canGoBack={false}
            onBack={q.reset}
            onChooseAnother={() => {
              q.reset();
              void chooseFiles();
            }}
          />
        );
      } else {
        screen = <SetupScreen q={q} />;
      }
      break;
    case "converting":
      screen = <ConvertingScreen q={q} />;
      break;
    case "finished": {
      const finished = state.items.filter((i) => i.status === "done" || i.status === "failed" || i.status === "cancelled");
      if (finished.length === 1 && finished[0].status === "done") {
        screen = <DoneScreen q={q} item={finished[0]} />;
      } else if (finished.length === 1) {
        key = "failed";
        screen = (
          <ErrorScreen
            error={finished[0].error ?? { code: "unknown", details: null }}
            canGoBack
            onBack={q.back}
            onChooseAnother={() => {
              q.reset();
              void chooseFiles();
            }}
          />
        );
      } else {
        screen = <BatchDoneScreen q={q} />;
      }
      break;
    }
  }

  return (
    <>
      <main className="flex h-full flex-col">
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={key}
            variants={screenVariants}
            initial="initial"
            animate="animate"
            exit="exit"
            className="flex min-h-0 flex-1 flex-col"
          >
            {key === "setup" ? screen : <Page center>{screen}</Page>}
          </motion.div>
        </AnimatePresence>
      </main>

      <button
        type="button"
        onClick={() => setSettingsOpen(true)}
        aria-label={t("app.settings")}
        title={t("app.settings")}
        className="fixed top-3 right-4 z-30 flex size-9 items-center justify-center rounded-full text-label-secondary transition-colors hover:bg-fill hover:text-label"
      >
        <Settings size={19} aria-hidden />
      </button>

      <SettingsSheet open={settingsOpen} onClose={() => setSettingsOpen(false)} />

      <Modal open={q.closeRequested} title={t("close.title")} onClose={q.dismissClose} size="alert">
        <p className="px-6 pt-2 text-[14px] leading-relaxed text-label-secondary">{t("close.message")}</p>
        <div className="flex justify-end gap-2 px-6 pt-5 pb-6">
          <Button onClick={q.dismissClose}>{t("close.keep")}</Button>
          <Button variant="primary" className="bg-danger hover:bg-danger" onClick={q.confirmClose}>
            {t("close.quit")}
          </Button>
        </div>
      </Modal>

      <DropOverlay visible={q.dragging && state.phase !== "empty" && state.phase !== "converting"} />
    </>
  );
}
