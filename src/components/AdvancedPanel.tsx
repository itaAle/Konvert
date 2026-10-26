import { AnimatePresence, motion } from "motion/react";
import { ChevronRight } from "lucide-react";
import { useId, useState, type ReactElement } from "react";

import type { AdvancedOptions } from "../bindings/AdvancedOptions";
import type { TargetOption } from "../bindings/TargetOption";
import { intlLocale, t } from "../i18n";
import { AUDIO_CODEC_NAMES, MAX_AUDIO_BITRATE, VIDEO_CODEC_NAMES } from "../lib/codecs";
import { formatSampleRate } from "../lib/format";
import { revealVariants } from "../lib/motion";
import { Button } from "./ui/Button";
import { Select, type SelectOption } from "./ui/Select";

const AUTO = "auto";
const AUDIO_BITRATES = [64, 96, 128, 160, 192, 256, 320];
const SAMPLE_RATES = [22050, 32000, 44100, 48000, 96000];
const VIDEO_BITRATES = [1000, 2500, 5000, 8000, 12000, 20000];
const HEIGHTS = [2160, 1440, 1080, 720, 480, 360];
const GIF_HEIGHTS = [720, 480, 360, 240];
const FPS = [60, 50, 30, 25, 24];
const GIF_FPS = [30, 24, 15, 10];
const IMAGE_QUALITIES = [100, 95, 90, 85, 80, 75, 70, 60, 50, 40];
const MAX_DIMENSIONS = [3840, 2560, 1920, 1280, 1024, 800];

const HEIGHT_LABELS: Record<number, string> = { 2160: "2160p (4K)", 1080: "1080p (Full HD)", 720: "720p (HD)" };

interface AdvancedPanelProps {
  option: TargetOption;
  value: AdvancedOptions;
  onChange: (value: AdvancedOptions) => void;
}

/** Collapsible "Advanced" section; every field defaults to automatic. */
export function AdvancedPanel({ option, value, onChange }: AdvancedPanelProps) {
  const [open, setOpen] = useState(false);
  const panelId = useId();
  const customized = Object.values(value).some((v) => v != null);

  const set = (patch: Partial<AdvancedOptions>) => {
    const next: AdvancedOptions = { ...value, ...patch };
    // Drop a sample rate the newly chosen encoder cannot use.
    const codec = option.audioCodecs.find((c) => c.codec === (next.audioCodec ?? option.audioCodecs[0]?.codec));
    if (next.sampleRate != null && codec?.sampleRates && !codec.sampleRates.includes(next.sampleRate)) {
      next.sampleRate = null;
    }
    onChange(next);
  };
  const num = (v: string) => (v === AUTO ? null : Number(v));
  const str = (v: number | null | undefined) => (v == null ? AUTO : String(v));

  // GIF, animated WebP and APNG: short loops with their own size and frame rate limits.
  const isGif = option.format === "gif" || option.format === "animatedWebp" || option.format === "apng";
  const isVideo = option.kind === "video";
  const audioCodec = option.audioCodecs.find((c) => c.codec === value.audioCodec) ?? option.audioCodecs[0];
  const autoLabel = t("advanced.auto");
  const originalLabel = t("advanced.original");

  const fields: ReactElement[] = [];

  if (option.kind === "image") {
    // Lossless formats and GIF (a palette) have no quality to tune.
    if (!option.lossless && option.format !== "stillGif") {
      fields.push(
        <Select key="iquality" label={t("advanced.imageQuality")} value={str(value.imageQuality)}
          options={[{ value: AUTO, label: autoLabel }, ...IMAGE_QUALITIES.map((q) => ({ value: String(q), label: String(q) }))]}
          onChange={(v) => set({ imageQuality: num(v) })} />,
      );
    }
    // An icon has fixed sizes.
    if (option.format !== "ico") {
      fields.push(
        <Select key="maxdim" label={t("advanced.maxDimension")} value={str(value.maxDimension)}
          options={[{ value: AUTO, label: originalLabel }, ...MAX_DIMENSIONS.map((d) => ({ value: String(d), label: t("advanced.pixels", { n: d }) }))]}
          onChange={(v) => set({ maxDimension: num(v) })} />,
      );
    }
  }
  if (isVideo && !isGif && option.videoCodecs.length > 1) {
    const opts: SelectOption<string>[] = [
      { value: AUTO, label: `${autoLabel} (${VIDEO_CODEC_NAMES[option.videoCodecs[0]]})` },
      ...option.videoCodecs.map((c) => ({ value: c, label: VIDEO_CODEC_NAMES[c] })),
    ];
    fields.push(
      <Select key="vcodec" label={t("advanced.videoCodec")} value={value.videoCodec ?? AUTO} options={opts}
        onChange={(v) => set({ videoCodec: v === AUTO ? null : (v as AdvancedOptions["videoCodec"]) })} />,
    );
  }
  if (isVideo) {
    const heights = isGif ? GIF_HEIGHTS : HEIGHTS;
    fields.push(
      <Select key="height" label={t("advanced.resolution")} value={str(value.maxHeight)}
        options={[{ value: AUTO, label: originalLabel }, ...heights.map((h) => ({ value: String(h), label: HEIGHT_LABELS[h] ?? `${h}p` }))]}
        onChange={(v) => set({ maxHeight: num(v) })} />,
      <Select key="fps" label={t("advanced.fps")} value={str(value.fps)}
        options={[{ value: AUTO, label: isGif ? autoLabel : originalLabel }, ...(isGif ? GIF_FPS : FPS).map((f) => ({ value: String(f), label: `${f} fps` }))]}
        onChange={(v) => set({ fps: num(v) })} />,
    );
  }
  if (isVideo && !isGif) {
    fields.push(
      <Select key="vbitrate" label={t("advanced.videoBitrate")} value={str(value.videoBitrateKbps)}
        options={[{ value: AUTO, label: autoLabel }, ...VIDEO_BITRATES.map((b) => ({
          value: String(b),
          label: t("advanced.mbps", { n: new Intl.NumberFormat(intlLocale(), { maximumFractionDigits: 1 }).format(b / 1000) }),
        }))]}
        onChange={(v) => set({ videoBitrateKbps: num(v) })} />,
    );
  }
  if (audioCodec) {
    if (option.audioCodecs.length > 1) {
      fields.push(
        <Select key="acodec" label={t("advanced.audioCodec")} value={value.audioCodec ?? AUTO}
          options={[
            { value: AUTO, label: `${autoLabel} (${AUDIO_CODEC_NAMES[option.audioCodecs[0].codec]})` },
            ...option.audioCodecs.map((c) => ({ value: c.codec, label: AUDIO_CODEC_NAMES[c.codec] })),
          ]}
          onChange={(v) => set({ audioCodec: v === AUTO ? null : (v as AdvancedOptions["audioCodec"]) })} />,
      );
    }
    if (!audioCodec.lossless) {
      const max = MAX_AUDIO_BITRATE[audioCodec.codec] ?? 320;
      fields.push(
        <Select key="abitrate" label={t("advanced.audioBitrate")} value={str(value.audioBitrateKbps)}
          options={[{ value: AUTO, label: autoLabel }, ...AUDIO_BITRATES.filter((b) => b <= max).map((b) => ({ value: String(b), label: t("advanced.kbps", { n: b }) }))]}
          onChange={(v) => set({ audioBitrateKbps: num(v) })} />,
      );
    }
    const rates = (audioCodec.sampleRates ?? SAMPLE_RATES).filter((r) => r >= 22050);
    fields.push(
      <Select key="rate" label={t("advanced.sampleRate")} value={str(value.sampleRate)}
        options={[{ value: AUTO, label: originalLabel }, ...rates.map((r) => ({ value: String(r), label: formatSampleRate(r) }))]}
        onChange={(v) => set({ sampleRate: num(v) })} />,
      <Select key="channels" label={t("advanced.channels")} value={value.channels ?? AUTO}
        options={[
          { value: AUTO, label: originalLabel },
          { value: "mono", label: t("file.channels.mono") },
          { value: "stereo", label: t("file.channels.stereo") },
        ]}
        onChange={(v) => set({ channels: v === AUTO ? null : (v as AdvancedOptions["channels"]) })} />,
    );
  }

  if (fields.length === 0) return null;

  return (
    <section>
      <button
        type="button"
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => setOpen((o) => !o)}
        className="flex items-center gap-1 rounded-full py-1 pr-3 pl-1 text-[15px] font-semibold hover:bg-fill"
      >
        <motion.span animate={{ rotate: open ? 90 : 0 }} className="inline-flex text-label-secondary">
          <ChevronRight size={17} aria-hidden />
        </motion.span>
        {t("advanced.title")}
        {customized && <span className="ml-1 size-1.5 rounded-full bg-accent" aria-hidden />}
      </button>
      <AnimatePresence initial={false}>
        {open && (
          <motion.div id={panelId} variants={revealVariants} initial="initial" animate="animate" exit="exit" className="overflow-hidden">
            <div className="grid grid-cols-2 gap-x-4 gap-y-3 px-1 pt-3">{fields}</div>
            {customized && (
              <Button variant="plain" size="sm" className="mt-3" onClick={() => onChange({})}>
                {t("advanced.reset")}
              </Button>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </section>
  );
}
