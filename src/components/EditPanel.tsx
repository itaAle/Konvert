import { FlipHorizontal2, FlipVertical2, Link2, Link2Off, RotateCcw, RotateCw } from "lucide-react";
import { useId, type ReactNode } from "react";

import { t } from "../i18n";
import {
  DEFAULT_EDIT,
  editedSize,
  flipCrop,
  turnCrop,
  turnedSize,
  type EditScope,
  type EditState,
  type ResizeMode,
  type Size,
} from "../state/edit";
import { Segmented } from "./ui/Segmented";
import { Switch } from "./ui/Switch";

interface EditPanelProps {
  value: EditState;
  onChange: (value: EditState) => void;
  scope: EditScope;
  /** Size of the only picture, as shown (`null`: several pictures). */
  size: Size | null;
}

const field =
  "h-9 w-full rounded-[10px] bg-fill px-3 text-[14px] text-label tabular-nums transition-colors placeholder:text-label-tertiary hover:bg-fill-strong focus:bg-fill-strong";

/** A whole number (empty: none). */
function SizeField({
  label,
  value,
  placeholder,
  suffix,
  onChange,
}: {
  label: string;
  value: number | null;
  placeholder?: string;
  suffix: string;
  onChange: (value: number | null) => void;
}) {
  const id = useId();
  return (
    <div className="flex min-w-0 flex-1 flex-col gap-1">
      <label htmlFor={id} className="px-1 text-[12px] font-medium text-label-secondary">
        {label}
      </label>
      <div className="relative">
        <input
          id={id}
          type="number"
          inputMode="numeric"
          min={1}
          value={value ?? ""}
          placeholder={placeholder}
          onChange={(e) => {
            if (e.target.value === "") return onChange(null);
            const v = Math.floor(Number(e.target.value));
            if (Number.isFinite(v) && v >= 1) onChange(v);
          }}
          className={`${field} pr-9`}
        />
        <span className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 text-[13px] text-label-tertiary">{suffix}</span>
      </div>
    </div>
  );
}

function IconButton({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className="flex size-9 items-center justify-center rounded-[10px] bg-fill text-label transition-colors hover:bg-fill-strong"
    >
      {children}
    </button>
  );
}

/**
 * The new size (a percentage, pixels, the longest side; never larger unless
 * asked) and the orientation (quarter turns, mirroring) of the pictures.
 */
export function EditPanel({ value, onChange, scope, size }: EditPanelProps) {
  const set = (patch: Partial<EditState>) => onChange({ ...value, ...patch });
  const result = size ? editedSize(size, value, scope) : null;
  // The picture as it is before the new size: turned and cropped.
  const before = size ? editedSize(size, { ...value, resize: "original" }, scope) : null;
  const turned = size ? turnedSize(size, scope.turn ? value.rotation : 0) : null;

  const turn = (clockwise: boolean) =>
    set({ rotation: (value.rotation + (clockwise ? 90 : 270)) % 360, crop: turnCrop(value.crop, clockwise) });
  const mirror = (horizontal: boolean) =>
    set(
      horizontal
        ? { flipHorizontal: !value.flipHorizontal, crop: flipCrop(value.crop, true) }
        : { flipVertical: !value.flipVertical, crop: flipCrop(value.crop, false) },
    );
  const oriented = value.rotation !== 0 || value.flipHorizontal || value.flipVertical;

  // With the proportions kept, one side is typed and the other follows.
  const auto = (side: "width" | "height") => {
    if (!value.keepRatio || !before) return t("edit.auto");
    const other = side === "width" ? value.height : value.width;
    if (other == null) return t("edit.auto");
    const ratio = before.width / before.height;
    return String(Math.round(side === "width" ? other * ratio : other / ratio));
  };

  return (
    <div className="flex flex-col gap-4 rounded-[14px] bg-surface px-4 py-3 shadow-card">
      {scope.resize && (
        <div className="flex flex-col gap-3">
          <Segmented<ResizeMode>
            label={t("edit.size")}
            value={value.resize}
            options={[
              { value: "original", label: t("edit.original") },
              { value: "percent", label: t("edit.percent") },
              { value: "pixels", label: t("edit.pixels") },
              { value: "longSide", label: t("edit.longSide") },
            ]}
            onChange={(resize) => set({ resize })}
          />
          {value.resize === "percent" && (
            <SizeField label={t("edit.percent")} value={value.percent} suffix="%" onChange={(percent) => set({ percent })} />
          )}
          {value.resize === "longSide" && (
            <SizeField
              label={t("edit.longSide")}
              value={value.side}
              placeholder={turned ? String(Math.max(turned.width, turned.height)) : undefined}
              suffix="px"
              onChange={(side) => set({ side })}
            />
          )}
          {value.resize === "pixels" && (
            <div className="flex items-end gap-2">
              <SizeField
                label={t("edit.width")}
                value={value.width}
                placeholder={auto("width")}
                suffix="px"
                onChange={(width) => set(value.keepRatio && width != null ? { width, height: null } : { width })}
              />
              <button
                type="button"
                aria-pressed={value.keepRatio}
                title={t("edit.keepRatio")}
                aria-label={t("edit.keepRatio")}
                onClick={() => set({ keepRatio: !value.keepRatio })}
                className={`mb-0 flex size-9 shrink-0 items-center justify-center rounded-[10px] transition-colors ${
                  value.keepRatio ? "bg-accent-soft text-accent" : "bg-fill text-label-secondary hover:bg-fill-strong"
                }`}
              >
                {value.keepRatio ? <Link2 size={17} aria-hidden /> : <Link2Off size={17} aria-hidden />}
              </button>
              <SizeField
                label={t("edit.height")}
                value={value.height}
                placeholder={auto("height")}
                suffix="px"
                onChange={(height) => set(value.keepRatio && height != null ? { height, width: null } : { height })}
              />
            </div>
          )}
          {value.resize !== "original" && (
            <div className="flex items-center justify-between gap-3">
              <span className="text-[13px] text-label-secondary">{t("edit.noEnlarge")}</span>
              <Switch checked={!value.enlarge} onChange={(on) => set({ enlarge: !on })} label={t("edit.noEnlarge")} />
            </div>
          )}
          <p className="px-1 text-[12px] leading-snug text-label-secondary" aria-live="polite">
            {result && before
              ? t("edit.result", {
                  from: `${before.width} × ${before.height}`,
                  to: `${result.width} × ${result.height}`,
                })
              : value.resize === "pixels" && !value.keepRatio
                ? t("edit.resultExact")
                : t("edit.resultMany")}
          </p>
        </div>
      )}
      {scope.turn && (
        <div className="flex flex-wrap items-center gap-2">
          <span className="mr-auto text-[13px] font-medium">{t("edit.orientation")}</span>
          <IconButton label={t("edit.rotateLeft")} onClick={() => turn(false)}>
            <RotateCcw size={17} aria-hidden />
          </IconButton>
          <IconButton label={t("edit.rotateRight")} onClick={() => turn(true)}>
            <RotateCw size={17} aria-hidden />
          </IconButton>
          {/* Lucide names its mirror icons after the axis: left-right mirroring shows a vertical one. */}
          <IconButton label={t("edit.flipH")} onClick={() => mirror(true)}>
            <FlipVertical2 size={17} aria-hidden />
          </IconButton>
          <IconButton label={t("edit.flipV")} onClick={() => mirror(false)}>
            <FlipHorizontal2 size={17} aria-hidden />
          </IconButton>
          {oriented && (
            <button
              type="button"
              onClick={() =>
                set({
                  rotation: DEFAULT_EDIT.rotation,
                  flipHorizontal: false,
                  flipVertical: false,
                  crop: null,
                })
              }
              className="rounded-full px-2 py-1 text-[13px] font-medium text-accent hover:bg-accent-soft"
            >
              {t("edit.reset")}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
