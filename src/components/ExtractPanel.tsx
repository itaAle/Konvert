import { Film, Music, Subtitles } from "lucide-react";
import { useId } from "react";

import type { ExtractTrack } from "../bindings/ExtractTrack";
import type { SubtitleFormat } from "../bindings/SubtitleFormat";
import type { TargetFormat } from "../bindings/TargetFormat";
import type { Track } from "../bindings/Track";
import { getLocale, t } from "../i18n";
import { codecName, formatChannels } from "../lib/format";
import { audioFormatFor, subtitleFormatFor } from "../state/extract";
import { languageName, normalLanguage } from "../state/tracks";
import { Segmented } from "./ui/Segmented";
import { Switch } from "./ui/Switch";

interface ExtractPanelProps {
  tracks: Track[];
  value: ExtractTrack[];
  onChange: (value: ExtractTrack[]) => void;
}

/** Audio formats a track can be saved in, by format id and name. */
const AUDIO_FORMATS: [TargetFormat, string][] = [
  ["m4a", "M4A (AAC)"],
  ["mp3", "MP3"],
  ["opus", "OPUS"],
  ["ogg", "OGG (Vorbis)"],
  ["flac", "FLAC"],
  ["wav", "WAV"],
  ["alac", "ALAC (M4A)"],
  ["ac3", "AC3"],
  ["eac3", "E-AC3"],
  ["mp2", "MP2"],
  ["wma", "WMA"],
];
const ICONS = { video: Film, audio: Music, subtitle: Subtitles } as const;

/**
 * Tracks chosen to become files of their own: each audio track in a format
 * (by default one that keeps it as it is), text subtitles as SRT, WebVTT or
 * ASS, the video in the file's own format without audio.
 */
export function ExtractPanel({ tracks, value, onChange }: ExtractPanelProps) {
  const locale = getLocale();
  const update = (index: number, patch: Partial<ExtractTrack>) =>
    onChange(value.map((e) => (e.index === index ? { ...e, ...patch } : e)));
  const toggle = (track: Track) =>
    onChange(
      value.some((e) => e.index === track.index)
        ? value.filter((e) => e.index !== track.index)
        : // In file order.
          [...value, { index: track.index, audio: null, subtitles: null }].sort(
            (a, b) => tracks.findIndex((t) => t.index === a.index) - tracks.findIndex((t) => t.index === b.index),
          ),
    );
  const counts = { video: 0, audio: 0, subtitle: 0 };

  return (
    <div className="flex flex-col divide-y divide-separator rounded-[14px] bg-surface shadow-card">
      {tracks.map((track) => {
        const number = ++counts[track.kind];
        const chosen = value.find((e) => e.index === track.index) ?? null;
        const Icon = ICONS[track.kind];
        const name = t(`tracks.${track.kind}`, { n: number });
        const details = [
          codecName(track.codec),
          track.channels ? formatChannels(track.channels) : null,
          track.language ? languageName(normalLanguage(track.language)!, locale) : null,
        ].filter(Boolean);
        return (
          <div key={track.index} className="flex flex-col gap-2 px-4 py-3">
            <div className="flex items-center gap-3">
              <Icon size={17} className="shrink-0 text-label-secondary" aria-hidden />
              <div className={`min-w-0 flex-1 ${track.bitmap ? "opacity-50" : ""}`}>
                <p className="truncate text-[14px] font-medium">
                  {name}
                  {track.title && <span className="font-normal text-label-secondary"> · {track.title}</span>}
                </p>
                <p className="truncate text-[12px] text-label-secondary">
                  {track.bitmap ? t("extract.pictures") : details.join(" · ")}
                </p>
              </div>
              <Switch
                checked={chosen != null}
                disabled={track.bitmap}
                onChange={() => toggle(track)}
                label={`${t("extract.save")}: ${name}`}
              />
            </div>
            {chosen && track.kind === "audio" && (
              <AudioFormat track={track} value={chosen.audio ?? null} onChange={(audio) => update(track.index, { audio })} />
            )}
            {chosen && track.kind === "subtitle" && (
              <div className="pl-8">
                <Segmented<SubtitleFormat>
                  label={t("extract.subtitleFormat")}
                  value={chosen.subtitles ?? subtitleFormatFor(track.codec)}
                  options={[
                    { value: "srt", label: "SRT" },
                    { value: "vtt", label: "WebVTT" },
                    { value: "ass", label: "ASS" },
                  ]}
                  onChange={(subtitles) => update(track.index, { subtitles })}
                />
              </div>
            )}
            {chosen && track.kind === "video" && (
              <p className="pl-8 text-[12px] text-label-secondary">{t("extract.videoOnly")}</p>
            )}
          </div>
        );
      })}
    </div>
  );
}

function AudioFormat({
  track,
  value,
  onChange,
}: {
  track: Track;
  value: TargetFormat | null;
  onChange: (value: TargetFormat | null) => void;
}) {
  const id = useId();
  const own = audioFormatFor(track.codec);
  const ownName = AUDIO_FORMATS.find(([f]) => f === own)?.[1] ?? own.toUpperCase();
  return (
    <div className="flex items-center gap-2 pl-8">
      <label htmlFor={id} className="text-[12px] text-label-secondary">
        {t("extract.format")}
      </label>
      <select
        id={id}
        value={value ?? ""}
        onChange={(e) => onChange((e.target.value || null) as TargetFormat | null)}
        className="h-8 min-w-0 flex-1 rounded-[8px] bg-fill px-2 text-[13px] text-label hover:bg-fill-strong"
      >
        <option value="">{t("extract.asIs", { format: ownName })}</option>
        {AUDIO_FORMATS.filter(([f]) => f !== own).map(([format, name]) => (
          <option key={format} value={format}>
            {name}
          </option>
        ))}
      </select>
    </div>
  );
}
