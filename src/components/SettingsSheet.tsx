import { motion } from "motion/react";
import { ExternalLink, LockKeyhole } from "lucide-react";
import { useEffect, useId, useState } from "react";

import appIcon from "../../src-tauri/icons/app-icon.svg";
import type { EngineInfo } from "../bindings/EngineInfo";
import type { LanguagePref } from "../bindings/LanguagePref";
import type { QualityPreset } from "../bindings/QualityPreset";
import type { ThemePref } from "../bindings/ThemePref";
import { t } from "../i18n";
import { backend } from "../lib/api";
import { spring } from "../lib/motion";
import { usePreferences } from "../state/preferences";
import { DestinationRow } from "./DestinationRow";
import { Button } from "./ui/Button";
import { Modal } from "./ui/Modal";
import { Select } from "./ui/Select";
import { Switch } from "./ui/Switch";

type Tab = "general" | "about";

/** `ffmpeg version n9.0.2-36-g… Copyright …` → `9.0.2`. */
export function shortFfmpegVersion(line: string): string {
  return line.match(/version n?(\d+(?:\.\d+)+)/)?.[1] ?? line.replace(/^ffmpeg version /, "").split(" ")[0];
}

function Row({ label, children, hint }: { label: string; children: React.ReactNode; hint?: string }) {
  return (
    <div className="flex flex-col gap-1.5">
      <p className="px-1 text-[12px] font-medium text-label-secondary">{label}</p>
      {children}
      {hint && <p className="px-1 text-[12px] leading-snug text-label-tertiary">{hint}</p>}
    </div>
  );
}

function General() {
  const { prefs, update } = usePreferences();
  const debugHintId = useId();
  return (
    <div className="flex flex-col gap-5">
      <div className="grid grid-cols-2 gap-4">
        <Select<LanguagePref>
          label={t("settings.language")}
          value={prefs.language}
          onChange={(language) => update({ language })}
          options={[
            { value: "auto", label: t("settings.languageAuto") },
            { value: "it", label: "Italiano" },
            { value: "en", label: "English" },
          ]}
        />
        <Select<ThemePref>
          label={t("settings.theme")}
          value={prefs.theme}
          onChange={(theme) => update({ theme })}
          options={[
            { value: "auto", label: t("settings.themeAuto") },
            { value: "light", label: t("settings.themeLight") },
            { value: "dark", label: t("settings.themeDark") },
          ]}
        />
        <Select<QualityPreset>
          label={t("settings.defaultPreset")}
          value={prefs.defaultPreset}
          onChange={(defaultPreset) => update({ defaultPreset })}
          options={[
            { value: "max", label: t("quality.max") },
            { value: "balanced", label: t("quality.balanced") },
            { value: "small", label: t("quality.small") },
          ]}
        />
      </div>
      <Row label={t("settings.destination")}>
        <div className="rounded-[10px] bg-fill px-3 py-1.5">
          <DestinationRow compact />
        </div>
      </Row>
      <div className="flex flex-col gap-2 rounded-[14px] bg-fill px-4 py-3">
        <div className="flex items-center justify-between gap-4">
          <p className="text-[14px] font-medium">{t("settings.debug")}</p>
          <Switch
            checked={prefs.debugLogging}
            onChange={(debugLogging) => update({ debugLogging })}
            label={t("settings.debug")}
            describedBy={debugHintId}
          />
        </div>
        <p id={debugHintId} className="text-[12px] leading-snug text-label-secondary">
          {t("settings.debugHint")}
        </p>
        <div className="-ml-3 flex gap-1">
          <Button variant="plain" size="sm" onClick={() => void backend.revealLog().catch(() => {})}>
            {t("settings.showLog")}
          </Button>
          <Button variant="plain" size="sm" onClick={() => void backend.clearLog().catch(() => {})}>
            {t("settings.clearLog")}
          </Button>
        </div>
      </div>
    </div>
  );
}

function About() {
  const [info, setInfo] = useState<EngineInfo | null>(null);
  useEffect(() => {
    backend.engineInfo().then(setInfo, () => {});
  }, []);
  return (
    <div className="flex flex-col items-center gap-5 text-center">
      <img src={appIcon} alt="" width={72} height={72} className="drop-shadow-md" />
      <div>
        <p className="text-[22px] font-semibold tracking-[-0.02em]">{t("app.name")}</p>
        <p className="mt-0.5 text-[13px] text-label-secondary">
          {info ? t("about.version", { version: info.appVersion }) : " "}
        </p>
        {info && (
          <p className="text-[12px] text-label-tertiary">
            {t("about.engine", { version: shortFfmpegVersion(info.ffmpegVersion) })}
          </p>
        )}
      </div>
      <div className="w-full rounded-[16px] bg-accent-soft px-5 py-4 text-left">
        <p className="flex items-center gap-2 text-[14px] font-semibold text-label">
          <LockKeyhole size={16} className="shrink-0 text-accent" aria-hidden />
          {t("about.privacy")}
        </p>
        <p className="mt-1.5 text-[12px] leading-snug text-label-secondary">{t("about.privacyDetails")}</p>
      </div>
      <p className="text-[12px] leading-snug text-label-secondary">{t("about.updates")}</p>
      <Button
        variant="secondary"
        icon={<ExternalLink size={15} aria-hidden />}
        onClick={() => void backend.openReleasesPage().catch(() => {})}
      >
        {t("about.releases")}
      </Button>
      <div className="flex flex-col items-center gap-1">
        <p className="text-[11px] text-label-tertiary">{t("about.license")}</p>
        <Button variant="plain" size="sm" onClick={() => void backend.openLicenses().catch(() => {})}>
          {t("about.licenses")}
        </Button>
      </div>
    </div>
  );
}

/** Settings and About, in a sheet with two tabs. */
export function SettingsSheet({ open, onClose, initialTab = "general" }: { open: boolean; onClose: () => void; initialTab?: Tab }) {
  const [tab, setTab] = useState<Tab>(initialTab);
  useEffect(() => {
    if (open) setTab(initialTab);
  }, [open, initialTab]);
  const tabs: { id: Tab; label: string }[] = [
    { id: "general", label: t("settings.general") },
    { id: "about", label: t("settings.about") },
  ];

  return (
    <Modal open={open} title={t("settings.title")} onClose={onClose}>
      <div className="flex items-center gap-3 border-b border-separator px-5 py-3">
        <div role="tablist" aria-label={t("settings.title")} className="flex flex-1 rounded-[10px] bg-fill p-[3px]">
          {tabs.map((item) => (
            <button
              key={item.id}
              type="button"
              role="tab"
              aria-selected={tab === item.id}
              onClick={() => setTab(item.id)}
              className={`relative h-8 flex-1 rounded-[8px] text-[13px] font-medium ${tab === item.id ? "text-label" : "text-label-secondary"}`}
            >
              {tab === item.id && (
                <motion.span
                  layoutId="settings-tab"
                  transition={spring}
                  className="absolute inset-0 rounded-[8px] bg-surface shadow-[0_1px_3px_rgb(0_0_0/0.12)]"
                />
              )}
              <span className="relative">{item.label}</span>
            </button>
          ))}
        </div>
        <Button variant="plain" size="sm" onClick={onClose}>
          {t("actions.done")}
        </Button>
      </div>
      <div role="tabpanel" className="overflow-y-auto px-6 py-5">
        {tab === "general" ? <General /> : <About />}
      </div>
    </Modal>
  );
}
