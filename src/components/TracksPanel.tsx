import { ChevronDown, ChevronUp, Film, Music, Star, Subtitles } from "lucide-react";
import { useId } from "react";

import type { Track } from "../bindings/Track";
import type { TrackChoice } from "../bindings/TrackChoice";
import { getLocale, t } from "../i18n";
import { codecName, formatChannels } from "../lib/format";
import {
  LANGUAGES,
  languageName,
  moveTrack,
  normalLanguage,
  setDefaultTrack,
  toggleTrack,
} from "../state/tracks";
import { Switch } from "./ui/Switch";

interface TracksPanelProps {
  tracks: Track[];
  /** The tracks kept, in order. */
  value: TrackChoice[];
  onChange: (value: TrackChoice[]) => void;
  /** Only audio tracks matter (an audio format). */
  audioOnly: boolean;
}

const ICONS = { video: Film, audio: Music, subtitle: Subtitles } as const;
const field =
  "h-8 min-w-0 rounded-[8px] bg-fill px-2 text-[13px] text-label transition-colors hover:bg-fill-strong focus:bg-fill-strong";

/**
 * Every track of the file: kept or left out, moved up or down within its
 * kind, the default one of each kind, its language and title.
 */
export function TracksPanel({ tracks, value, onChange, audioOnly }: TracksPanelProps) {
  const locale = getLocale();
  const shown = tracks.filter((track) => !audioOnly || track.kind === "audio");
  // Kept tracks in their chosen order, then those left out in file order.
  const kept = value.map((c) => shown.find((t) => t.index === c.index)).filter((t): t is Track => t != null);
  const rows = [...kept, ...shown.filter((t) => !kept.includes(t))];
  const byKind = (kind: Track["kind"]) => rows.filter((t) => t.kind === kind);

  const update = (index: number, patch: Partial<TrackChoice>) =>
    onChange(value.map((c) => (c.index === index ? { ...c, ...patch } : c)));

  return (
    <div className="flex flex-col divide-y divide-separator rounded-[14px] bg-surface shadow-card">
      {(["video", "audio", "subtitle"] as const).flatMap((kind) =>
        byKind(kind).map((track, position) => {
          const choice = value.find((c) => c.index === track.index);
          const keptOfKind = kept.filter((k) => k.kind === kind);
          const place = keptOfKind.indexOf(track);
          return (
            <TrackRow
              key={track.index}
              track={track}
              number={position + 1}
              choice={choice ?? null}
              locale={locale}
              canUp={place > 0}
              canDown={place >= 0 && place < keptOfKind.length - 1}
              onToggle={() => onChange(toggleTrack(value, tracks, track.index))}
              onMove={(delta) => onChange(moveTrack(value, tracks, track.index, delta))}
              onDefault={() => onChange(setDefaultTrack(value, tracks, track.index))}
              onLanguage={(language) => update(track.index, { language })}
              onTitle={(title) => update(track.index, { title })}
            />
          );
        }),
      )}
    </div>
  );
}

function TrackRow({
  track,
  number,
  choice,
  locale,
  canUp,
  canDown,
  onToggle,
  onMove,
  onDefault,
  onLanguage,
  onTitle,
}: {
  track: Track;
  number: number;
  choice: TrackChoice | null;
  locale: string;
  canUp: boolean;
  canDown: boolean;
  onToggle: () => void;
  onMove: (delta: -1 | 1) => void;
  onDefault: () => void;
  onLanguage: (language: string | null) => void;
  onTitle: (title: string | null) => void;
}) {
  const Icon = ICONS[track.kind];
  const name = t(`tracks.${track.kind}`, { n: number });
  const details = [
    codecName(track.codec),
    track.channels ? formatChannels(track.channels) : null,
    track.bitmap ? t("tracks.pictures") : null,
  ].filter(Boolean);
  const language = normalLanguage(choice?.language ?? null);
  const languageId = useId();
  const titleId = useId();
  const listed = LANGUAGES.map(([code]) => code);
  const options = language && !listed.includes(language) ? [language, ...listed] : listed;
  const kept = choice != null;
  return (
    <div className="flex flex-col gap-2 px-4 py-3">
      <div className="flex items-center gap-3">
        <Icon size={17} className={`shrink-0 ${kept ? "text-label-secondary" : "text-label-tertiary"}`} aria-hidden />
        <div className={`min-w-0 flex-1 ${kept ? "" : "opacity-50"}`}>
          <p className="truncate text-[14px] font-medium">
            {name}
            {track.title && <span className="font-normal text-label-secondary"> · {track.title}</span>}
          </p>
          <p className="truncate text-[12px] text-label-secondary">
            {[...details, track.language ? languageName(normalLanguage(track.language)!, locale) : null].filter(Boolean).join(" · ")}
          </p>
        </div>
        {kept && (
          <>
            <button
              type="button"
              aria-pressed={choice.default}
              title={t("tracks.default")}
              aria-label={`${t("tracks.default")}: ${name}`}
              onClick={onDefault}
              className={`flex size-8 items-center justify-center rounded-[8px] transition-colors ${
                choice.default ? "text-accent" : "text-label-tertiary hover:bg-fill"
              }`}
            >
              <Star size={16} fill={choice.default ? "currentColor" : "none"} aria-hidden />
            </button>
            <button
              type="button"
              disabled={!canUp}
              aria-label={`${t("tracks.up")}: ${name}`}
              onClick={() => onMove(-1)}
              className="flex size-8 items-center justify-center rounded-[8px] text-label-secondary hover:bg-fill disabled:opacity-30"
            >
              <ChevronUp size={16} aria-hidden />
            </button>
            <button
              type="button"
              disabled={!canDown}
              aria-label={`${t("tracks.down")}: ${name}`}
              onClick={() => onMove(1)}
              className="flex size-8 items-center justify-center rounded-[8px] text-label-secondary hover:bg-fill disabled:opacity-30"
            >
              <ChevronDown size={16} aria-hidden />
            </button>
          </>
        )}
        <Switch checked={kept} onChange={onToggle} label={`${t("tracks.keep")}: ${name}`} />
      </div>
      {kept && (
        <div className="flex gap-2 pl-8">
          <label htmlFor={languageId} className="sr-only">
            {t("tracks.language")}
          </label>
          <select
            id={languageId}
            value={language ?? ""}
            onChange={(e) => onLanguage(e.target.value || null)}
            className={`${field} w-40`}
          >
            <option value="">{t("tracks.noLanguage")}</option>
            {options.map((code) => (
              <option key={code} value={code}>
                {languageName(code, locale)}
              </option>
            ))}
          </select>
          <label htmlFor={titleId} className="sr-only">
            {t("tracks.name")}
          </label>
          <input
            id={titleId}
            value={choice.title ?? ""}
            placeholder={t("tracks.name")}
            onChange={(e) => onTitle(e.target.value || null)}
            className={`${field} flex-1`}
          />
        </div>
      )}
    </div>
  );
}
