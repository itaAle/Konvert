import { FileAudio, X } from "lucide-react";
import { useId, useState } from "react";

import type { AudioFile } from "../bindings/AudioFile";
import { getLocale, t } from "../i18n";
import { backend } from "../lib/api";
import { codecName, formatChannels } from "../lib/format";
import { LANGUAGES, languageName } from "../state/tracks";
import { Button } from "./ui/Button";
import { Segmented } from "./ui/Segmented";

interface AudioFilePanelProps {
  value: AudioFile | null;
  onChange: (value: AudioFile | null) => void;
}

const field =
  "h-9 rounded-[10px] bg-fill px-3 text-[14px] text-label tabular-nums transition-colors hover:bg-fill-strong focus:bg-fill-strong";

/**
 * The audio of another file (a song, a dubbing, a voice-over) added to the
 * video or replacing its audio, moved earlier or later, with its language.
 */
export function AudioFilePanel({ value, onChange }: AudioFilePanelProps) {
  const [error, setError] = useState(false);
  const offsetId = useId();
  const languageId = useId();
  const locale = getLocale();

  const choose = async () => {
    const path = await backend.pickAudioFile(t("audioFile.pickerTitle"), t("audioFile.pickerFilter"));
    if (!path) return;
    try {
      const file = await backend.analyzeFile(path);
      const audio = file.media.audio[0];
      if (!audio) return setError(true);
      setError(false);
      onChange({
        path,
        codec: audio.codec,
        channels: audio.channels,
        replace: value?.replace ?? false,
        offsetMs: value?.offsetMs ?? 0,
        language: file.media.tracks.find((tr) => tr.index === audio.index)?.language ?? null,
      });
    } catch {
      setError(true);
    }
  };

  const name = value ? (value.path.split(/[\\/]/).pop() ?? value.path) : null;
  return (
    <div className="flex flex-col gap-3 rounded-[14px] bg-surface px-4 py-3 shadow-card">
      {value ? (
        <>
          <div className="flex items-center gap-3">
            <FileAudio size={17} className="shrink-0 text-label-secondary" aria-hidden />
            <div className="min-w-0 flex-1">
              <p className="truncate text-[14px] font-medium" title={value.path}>
                {name}
              </p>
              <p className="text-[12px] text-label-secondary">
                {[codecName(value.codec), value.channels ? formatChannels(value.channels) : null].filter(Boolean).join(" · ")}
              </p>
            </div>
            <button
              type="button"
              aria-label={t("audioFile.remove")}
              title={t("audioFile.remove")}
              onClick={() => onChange(null)}
              className="flex size-8 items-center justify-center rounded-[8px] text-label-secondary hover:bg-fill"
            >
              <X size={16} aria-hidden />
            </button>
          </div>
          <Segmented<"add" | "replace">
            label={t("audioFile.mode")}
            value={value.replace ? "replace" : "add"}
            options={[
              { value: "add", label: t("audioFile.add") },
              { value: "replace", label: t("audioFile.replace") },
            ]}
            onChange={(mode) => onChange({ ...value, replace: mode === "replace" })}
          />
          <div className="flex items-end gap-3">
            <div className="flex flex-1 flex-col gap-1">
              <label htmlFor={offsetId} className="px-1 text-[12px] font-medium text-label-secondary">
                {t("audioFile.offset")}
              </label>
              <input
                id={offsetId}
                type="number"
                step={10}
                value={value.offsetMs}
                onChange={(e) => {
                  const v = Math.round(Number(e.target.value));
                  if (Number.isFinite(v)) onChange({ ...value, offsetMs: Math.max(-3_600_000, Math.min(3_600_000, v)) });
                }}
                className={`${field} w-full`}
              />
            </div>
            <div className="flex flex-1 flex-col gap-1">
              <label htmlFor={languageId} className="px-1 text-[12px] font-medium text-label-secondary">
                {t("tracks.language")}
              </label>
              <select
                id={languageId}
                value={value.language ?? ""}
                onChange={(e) => onChange({ ...value, language: e.target.value || null })}
                className={`${field} w-full`}
              >
                <option value="">{t("tracks.noLanguage")}</option>
                {LANGUAGES.map(([code]) => (
                  <option key={code} value={code}>
                    {languageName(code, locale)}
                  </option>
                ))}
              </select>
            </div>
          </div>
          <p className="px-1 text-[12px] leading-snug text-label-secondary">{t("audioFile.offsetHint")}</p>
        </>
      ) : (
        <div className="flex items-center justify-between gap-3">
          <p className="text-[13px] leading-snug text-label-secondary">{t("audioFile.hint")}</p>
          <Button variant="plain" size="sm" onClick={() => void choose()}>
            {t("audioFile.choose")}
          </Button>
        </div>
      )}
      {error && <p className="px-1 text-[12px] text-warning">{t("audioFile.noAudio")}</p>}
    </div>
  );
}
