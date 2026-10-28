import { Hash, ImagePlus, Stamp } from "lucide-react";
import { useId, type ReactNode } from "react";

import type { Anchor } from "../bindings/Anchor";
import type { NumberFormat } from "../bindings/NumberFormat";
import type { PageNumbers } from "../bindings/PageNumbers";
import type { Watermark } from "../bindings/Watermark";
import { t } from "../i18n";
import { chosenPages } from "../state/stamps";
import { Button } from "./ui/Button";
import { Segmented } from "./ui/Segmented";
import { Select } from "./ui/Select";
import { Switch } from "./ui/Switch";

/** A gray, light watermark across the middle of the page, at 45 degrees. */
export const DEFAULT_WATERMARK: Watermark = {
  text: "",
  picture: null,
  size: 60,
  color: "#808080",
  opacity: 0.3,
  angle: 45,
  position: "center",
  pages: null,
};

/** Plain numbers at the bottom center of every page. */
export const DEFAULT_NUMBERS: PageNumbers = {
  position: "bottom",
  format: "plain",
  word: null,
  size: 10,
  first: 1,
  start: 1,
  skip: null,
};

const COLORS = [
  { value: "#808080", key: "stamps.gray" },
  { value: "#c62828", key: "stamps.red" },
  { value: "#1565c0", key: "stamps.blue" },
  { value: "#000000", key: "stamps.black" },
] as const;

const GRID: Anchor[] = ["topLeft", "top", "topRight", "left", "center", "right", "bottomLeft", "bottom", "bottomRight"];

interface StampPanelProps {
  watermark: Watermark | null;
  numbers: PageNumbers | null;
  onWatermark: (value: Watermark | null) => void;
  onNumbers: (value: PageNumbers | null) => void;
  /** Asks for a PNG or JPEG picture (`null`: none chosen). */
  pickPicture: () => Promise<string | null>;
}

/** A watermark and page numbers drawn over the pages of the new PDF. */
export function StampPanel({ watermark, numbers, onWatermark, onNumbers, pickPicture }: StampPanelProps) {
  const pagesId = useId();
  const textId = useId();
  const skipId = useId();
  const firstId = useId();
  const startId = useId();
  const w = watermark;
  const n = numbers;
  const set = (patch: Partial<Watermark>) => w && onWatermark({ ...w, ...patch });
  const setN = (patch: Partial<PageNumbers>) => n && onNumbers({ ...n, ...patch });
  const pagesInvalid = w != null && chosenPages(w.pages, 1) == null;
  const skipInvalid = n != null && chosenPages(n.skip, 1) == null;
  const picture = w?.picture != null;
  const pictureName = w?.picture?.split(/[\\/]/).pop();
  const field = "h-9 w-full rounded-[10px] bg-fill px-3 text-[14px] text-label transition-colors placeholder:text-label-tertiary hover:bg-fill-strong focus:bg-fill-strong";

  return (
    <div className="flex flex-col gap-3">
      <Card
        icon={<Stamp size={17} aria-hidden />}
        title={t("stamps.watermark")}
        hint={t("stamps.watermarkHint")}
        on={w != null}
        onToggle={(on) => onWatermark(on ? DEFAULT_WATERMARK : null)}
      >
        {w && (
          <>
            <Segmented<"text" | "picture">
              label={t("stamps.kind")}
              value={picture ? "picture" : "text"}
              options={[
                { value: "text", label: t("stamps.text") },
                { value: "picture", label: t("stamps.picture") },
              ]}
              onChange={async (kind) => {
                if (kind === "text") return set({ picture: null, size: 60 });
                const path = await pickPicture();
                if (path) set({ picture: path, size: 50, angle: 0 });
              }}
            />
            {picture ? (
              <div className="flex items-center gap-2">
                <ImagePlus size={16} className="shrink-0 text-label-secondary" aria-hidden />
                <p className="min-w-0 flex-1 truncate text-[13px]" title={w.picture ?? undefined}>
                  {pictureName}
                </p>
                <Button
                  size="sm"
                  onClick={async () => {
                    const path = await pickPicture();
                    if (path) set({ picture: path });
                  }}
                >
                  {t("stamps.choosePicture")}
                </Button>
              </div>
            ) : (
              <div className="flex flex-col gap-1">
                <label htmlFor={textId} className="px-1 text-[12px] font-medium text-label-secondary">
                  {t("stamps.textLabel")}
                </label>
                <input
                  id={textId}
                  type="text"
                  autoComplete="off"
                  value={w.text}
                  placeholder={t("stamps.textPlaceholder")}
                  onChange={(e) => set({ text: e.target.value })}
                  className={field}
                />
              </div>
            )}
            <div className="grid grid-cols-2 gap-3">
              <Select
                label={t("stamps.size")}
                value={String(w.size)}
                options={(picture ? [25, 50, 75] : [36, 60, 96]).map((v, i) => ({
                  value: String(v),
                  label: t(["stamps.small", "stamps.medium", "stamps.large"][i] as "stamps.small"),
                }))}
                onChange={(v) => set({ size: Number(v) })}
              />
              {!picture && (
                <Select
                  label={t("stamps.color")}
                  value={w.color}
                  options={COLORS.map((c) => ({ value: c.value, label: t(c.key) }))}
                  onChange={(color) => set({ color })}
                />
              )}
            </div>
            <Segmented<string>
              label={t("stamps.opacity")}
              value={String(w.opacity)}
              options={[
                { value: "0.15", label: t("stamps.light") },
                { value: "0.3", label: t("stamps.normal") },
                { value: "0.6", label: t("stamps.strong") },
              ]}
              onChange={(v) => set({ opacity: Number(v) })}
            />
            <Segmented<string>
              label={t("stamps.angle")}
              value={String(w.angle)}
              options={[
                { value: "0", label: t("stamps.horizontal") },
                { value: "45", label: t("stamps.diagonal") },
                { value: "90", label: t("stamps.vertical") },
              ]}
              onChange={(v) => set({ angle: Number(v) })}
            />
            <Positions label={t("stamps.position")} value={w.position} options={GRID} onChange={(position) => set({ position })} />
            <div className="flex flex-col gap-1">
              <label htmlFor={pagesId} className="px-1 text-[12px] font-medium text-label-secondary">
                {t("stamps.pages")}
              </label>
              <input
                id={pagesId}
                type="text"
                inputMode="numeric"
                autoComplete="off"
                value={w.pages ?? ""}
                placeholder={t("stamps.pagesPlaceholder")}
                aria-invalid={pagesInvalid}
                onChange={(e) => set({ pages: e.target.value.trim() ? e.target.value : null })}
                className={`${field} ${pagesInvalid ? "ring-2 ring-warning" : ""}`}
              />
            </div>
          </>
        )}
      </Card>

      <Card
        icon={<Hash size={17} aria-hidden />}
        title={t("stamps.numbers")}
        hint={t("stamps.numbersHint")}
        on={n != null}
        onToggle={(on) => onNumbers(on ? { ...DEFAULT_NUMBERS, word: t("stamps.pageWord") } : null)}
      >
        {n && (
          <>
            <Positions
              label={t("stamps.position")}
              value={n.position}
              options={["topLeft", "top", "topRight", "bottomLeft", "bottom", "bottomRight"]}
              onChange={(position) => setN({ position })}
            />
            <div className="grid grid-cols-2 gap-3">
              <Select<NumberFormat>
                label={t("stamps.format")}
                value={n.format}
                options={[
                  { value: "plain", label: t("stamps.plain") },
                  { value: "ofTotal", label: t("stamps.ofTotal") },
                  { value: "word", label: t("stamps.word") },
                ]}
                onChange={(format) => setN({ format, word: t("stamps.pageWord") })}
              />
              <Select
                label={t("stamps.size")}
                value={String(n.size)}
                options={[9, 11, 14].map((v, i) => ({
                  value: String(v),
                  label: t(["stamps.small", "stamps.medium", "stamps.large"][i] as "stamps.small"),
                }))}
                onChange={(v) => setN({ size: Number(v) })}
              />
            </div>
            <div className="grid grid-cols-3 gap-3">
              <NumberField id={firstId} label={t("stamps.first")} value={n.first} onChange={(first) => setN({ first })} />
              <NumberField id={startId} label={t("stamps.start")} value={n.start} onChange={(start) => setN({ start })} />
              <div className="flex flex-col gap-1">
                <label htmlFor={skipId} className="px-1 text-[12px] font-medium text-label-secondary">
                  {t("stamps.skip")}
                </label>
                <input
                  id={skipId}
                  type="text"
                  inputMode="numeric"
                  autoComplete="off"
                  value={n.skip ?? ""}
                  placeholder={t("stamps.skipPlaceholder")}
                  aria-invalid={skipInvalid}
                  onChange={(e) => setN({ skip: e.target.value.trim() ? e.target.value : null })}
                  className={`${field} ${skipInvalid ? "ring-2 ring-warning" : ""}`}
                />
              </div>
            </div>
          </>
        )}
      </Card>
      {(pagesInvalid || skipInvalid) && (
        <p role="alert" className="px-1 text-[12px] text-warning">
          {t("stamps.pagesInvalid")}
        </p>
      )}
    </div>
  );
}

function Card({
  icon,
  title,
  hint,
  on,
  onToggle,
  children,
}: {
  icon: ReactNode;
  title: string;
  hint: string;
  on: boolean;
  onToggle: (on: boolean) => void;
  children: ReactNode;
}) {
  const hintId = useId();
  return (
    <div className="flex flex-col gap-3 rounded-[14px] bg-surface px-4 py-3 shadow-card">
      <div className="flex items-start gap-3">
        <span className="mt-0.5 shrink-0 text-label-secondary">{icon}</span>
        <div className="min-w-0 flex-1">
          <p className="text-[14px] font-medium">{title}</p>
          <p id={hintId} className="mt-0.5 text-[12px] leading-snug text-label-secondary">
            {hint}
          </p>
        </div>
        <Switch checked={on} onChange={onToggle} label={title} describedBy={hintId} />
      </div>
      {on && <div className="flex flex-col gap-3 pl-8">{children}</div>}
    </div>
  );
}

/** Where on the page: a small grid of the places offered. */
function Positions({ label, value, options, onChange }: { label: string; value: Anchor; options: Anchor[]; onChange: (value: Anchor) => void }) {
  return (
    <div className="flex items-center gap-3">
      <p className="min-w-0 flex-1 px-1 text-[12px] font-medium text-label-secondary">{label}</p>
      <div role="radiogroup" aria-label={label} className="grid grid-cols-3 gap-1 rounded-[10px] bg-fill p-1.5">
        {options.map((anchor) => (
          <button
            key={anchor}
            type="button"
            role="radio"
            aria-checked={value === anchor}
            aria-label={t(`stamps.at.${anchor}` as "stamps.at.center")}
            title={t(`stamps.at.${anchor}` as "stamps.at.center")}
            onClick={() => onChange(anchor)}
            className={`size-5 rounded-[5px] transition-colors ${value === anchor ? "bg-accent" : "bg-fill-strong hover:bg-label-tertiary"}`}
          />
        ))}
      </div>
    </div>
  );
}

function NumberField({ id, label, value, onChange }: { id: string; label: string; value: number; onChange: (value: number) => void }) {
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="px-1 text-[12px] font-medium text-label-secondary">
        {label}
      </label>
      <input
        id={id}
        type="number"
        min={1}
        value={value}
        onChange={(e) => {
          const v = Math.floor(Number(e.target.value));
          if (Number.isFinite(v) && v >= 1) onChange(v);
        }}
        className="h-9 w-full rounded-[10px] bg-fill px-3 text-[14px] text-label tabular-nums transition-colors hover:bg-fill-strong focus:bg-fill-strong"
      />
    </div>
  );
}
