// User preferences (theme, language, default preset, destination, debug log):
// loaded once from the backend, saved on every change, applied immediately.
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import type { Preferences } from "../bindings/Preferences";
import { resolveLocale, setLocale, type Locale } from "../i18n";
import { backend } from "../lib/api";

export const DEFAULT_PREFERENCES: Preferences = {
  language: "auto",
  theme: "auto",
  defaultPreset: "balanced",
  outputDir: null,
  debugLogging: false,
};

interface PreferencesContext {
  prefs: Preferences;
  /** `true` once the saved preferences have been read. */
  loaded: boolean;
  locale: Locale;
  update(patch: Partial<Preferences>): void;
}

const Context = createContext<PreferencesContext | null>(null);

export function PreferencesProvider({ children }: { children: ReactNode }) {
  const [prefs, setPrefs] = useState(DEFAULT_PREFERENCES);
  const [loaded, setLoaded] = useState(false);
  const current = useRef(prefs);

  useEffect(() => {
    backend
      .getPreferences()
      .then((saved) => {
        current.current = saved;
        setPrefs(saved);
      })
      .catch(() => {})
      .finally(() => setLoaded(true));
  }, []);

  // Applied during render so every string of this render uses the right language.
  const locale = resolveLocale(prefs.language);
  useMemo(() => setLocale(locale), [locale]);

  // Light/dark: explicit choice, or follow the system (and its changes).
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = prefs.theme === "dark" || (prefs.theme === "auto" && media.matches);
      document.documentElement.dataset.scheme = dark ? "dark" : "light";
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [prefs.theme]);

  const update = useCallback((patch: Partial<Preferences>) => {
    const next = { ...current.current, ...patch };
    current.current = next;
    setPrefs(next);
    void backend.setPreferences(next).catch(() => {});
  }, []);

  const value = useMemo(() => ({ prefs, loaded, locale, update }), [prefs, loaded, locale, update]);
  return <Context.Provider value={value}>{children}</Context.Provider>;
}

export function usePreferences(): PreferencesContext {
  const ctx = useContext(Context);
  if (!ctx) throw new Error("usePreferences outside PreferencesProvider");
  return ctx;
}
