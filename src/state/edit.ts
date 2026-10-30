import type { CropRect } from "../bindings/CropRect";
import type { PictureEdit } from "../bindings/PictureEdit";
import type { Resize } from "../bindings/Resize";

/** How the new size is given. */
export type ResizeMode = "original" | "percent" | "pixels" | "longSide";

/** Proportions of the crop. */
export type CropRatio = "free" | "original" | "1:1" | "4:3" | "3:2" | "16:9";

/** The picture changes chosen in the setup screen. */
export interface EditState {
  /** Clockwise turn: 0, 90, 180 or 270. */
  rotation: number;
  flipHorizontal: boolean;
  flipVertical: boolean;
  /** Part kept, as fractions of the turned picture (one photo only). */
  crop: CropRect | null;
  cropRatio: CropRatio;
  /** A fixed ratio is taller than wide. */
  cropPortrait: boolean;
  resize: ResizeMode;
  percent: number | null;
  width: number | null;
  height: number | null;
  keepRatio: boolean;
  side: number | null;
  /** The picture may become larger than it is ("Don't enlarge" off). */
  enlarge: boolean;
}

export const DEFAULT_EDIT: EditState = {
  rotation: 0,
  flipHorizontal: false,
  flipVertical: false,
  crop: null,
  cropRatio: "free",
  cropPortrait: false,
  resize: "original",
  percent: 50,
  width: null,
  height: null,
  keepRatio: true,
  side: null,
  enlarge: false,
};

/** Smallest side of a crop, as a fraction of the picture. */
export const MIN_CROP = 0.02;

const positive = (v: number | null): v is number => v != null && Number.isFinite(v) && v > 0;

/** The new size as the core expects it (`null`: the size does not change). */
export function resizeOf(edit: EditState): Resize | null {
  switch (edit.resize) {
    case "percent":
      return positive(edit.percent) ? { mode: "percent", percent: edit.percent } : null;
    case "pixels": {
      const width = positive(edit.width) ? Math.round(edit.width) : null;
      const height = positive(edit.height) ? Math.round(edit.height) : null;
      return width != null || height != null ? { mode: "pixels", width, height, keepRatio: edit.keepRatio } : null;
    }
    case "longSide":
      return positive(edit.side) ? { mode: "longSide", side: Math.round(edit.side) } : null;
    default:
      return null;
  }
}

/** What applies to these pictures. */
export interface EditScope {
  /** Turning, mirroring (not for pictures taken from videos). */
  turn: boolean;
  /** Cropping (one photo only). */
  crop: boolean;
  /** A new size (not for icons, whose sizes are fixed). */
  resize: boolean;
}

/** The edit sent to the core (`null`: nothing changes and no copy is asked). */
export function pictureEditOf(edit: EditState, scope: EditScope, copy: string | null): PictureEdit | null {
  const result: PictureEdit = {
    rotation: scope.turn ? edit.rotation % 360 : 0,
    flipHorizontal: scope.turn && edit.flipHorizontal,
    flipVertical: scope.turn && edit.flipVertical,
    crop: scope.crop ? edit.crop : null,
    resize: scope.resize ? resizeOf(edit) : null,
    enlarge: edit.enlarge,
    copy: copy != null,
    label: copy,
  };
  return copy != null || changes(result) ? result : null;
}

function changes(edit: PictureEdit): boolean {
  return edit.rotation !== 0 || edit.flipHorizontal || edit.flipVertical || edit.crop != null || edit.resize != null;
}

/** Something would change in the picture. */
export function editChanged(edit: EditState, scope: EditScope): boolean {
  return changes(pictureEditOf(edit, scope, "") as PictureEdit);
}

export interface Size {
  width: number;
  height: number;
}

/** Size of a picture after its turn. */
export function turnedSize(size: Size, rotation: number): Size {
  return rotation % 180 === 90 ? { width: size.height, height: size.width } : size;
}

/**
 * Size of the edited picture, by the same rules as the core (turn, crop,
 * new size, never larger unless asked); the format's own limits are left to
 * the core.
 */
export function editedSize(source: Size, edit: EditState, scope: EditScope): Size & { stretched: boolean } {
  const turned = scope.turn ? turnedSize(source, edit.rotation) : source;
  const crop = scope.crop ? edit.crop : null;
  const w = crop ? Math.max(1, crop.width * turned.width) : turned.width;
  const h = crop ? Math.max(1, crop.height * turned.height) : turned.height;
  const resize = scope.resize ? resizeOf(edit) : null;
  const limit = (factor: number) => (edit.enlarge ? factor : Math.min(factor, 1));
  const whole = (width: number, height: number, stretched = false) => ({
    width: Math.max(1, Math.round(width)),
    height: Math.max(1, Math.round(height)),
    stretched,
  });
  if (!resize) return whole(w, h);
  if (resize.mode === "percent") {
    const f = limit(resize.percent / 100);
    return whole(w * f, h * f);
  }
  if (resize.mode === "longSide") {
    const f = limit(resize.side / Math.max(w, h));
    return whole(w * f, h * f);
  }
  if (resize.keepRatio) {
    const factors = [resize.width != null ? resize.width / w : null, resize.height != null ? resize.height / h : null].filter(
      (f): f is number => f != null,
    );
    const f = limit(Math.min(...factors));
    return whole(w * f, h * f);
  }
  const side = (wanted: number | null, own: number) => (wanted == null ? own : edit.enlarge ? wanted : Math.min(wanted, own));
  const nw = side(resize.width, w);
  const nh = side(resize.height, h);
  return whole(nw, nh, Math.abs(nw / nh / (w / h) - 1) > 0.01);
}

/** The crop after the picture turns a quarter more (`clockwise` or not). */
export function turnCrop(crop: CropRect | null, clockwise: boolean): CropRect | null {
  if (!crop) return null;
  const { x, y, width, height } = crop;
  return clockwise
    ? { x: 1 - y - height, y: x, width: height, height: width }
    : { x: y, y: 1 - x - width, width: height, height: width };
}

/** The crop after the picture is mirrored. */
export function flipCrop(crop: CropRect | null, horizontal: boolean): CropRect | null {
  if (!crop) return null;
  return horizontal ? { ...crop, x: 1 - crop.x - crop.width } : { ...crop, y: 1 - crop.y - crop.height };
}

/** Width over height (in pixels) of a fixed crop ratio (`null`: free). */
export function ratioValue(ratio: CropRatio, portrait: boolean, shown: Size): number | null {
  if (ratio === "free") return null;
  if (ratio === "original") return shown.width / shown.height;
  const [a, b] = ratio.split(":").map(Number);
  return portrait ? b / a : a / b;
}

/** The largest crop with these proportions, centered on `around` (or the picture). */
export function cropWithRatio(ratio: number, shown: Size, around: CropRect | null = null): CropRect {
  // Width over height of the crop in fractions of the picture.
  const r = (ratio * shown.height) / shown.width;
  let width = 1;
  let height = 1 / r;
  if (height > 1) {
    height = 1;
    width = r;
  }
  const cx = around ? around.x + around.width / 2 : 0.5;
  const cy = around ? around.y + around.height / 2 : 0.5;
  return {
    x: Math.min(Math.max(cx - width / 2, 0), 1 - width),
    y: Math.min(Math.max(cy - height / 2, 0), 1 - height),
    width,
    height,
  };
}

/** A crop kept inside the picture, not smaller than `MIN_CROP`. */
export function clampCrop(crop: CropRect): CropRect {
  const width = Math.min(Math.max(crop.width, MIN_CROP), 1);
  const height = Math.min(Math.max(crop.height, MIN_CROP), 1);
  return {
    x: Math.min(Math.max(crop.x, 0), 1 - width),
    y: Math.min(Math.max(crop.y, 0), 1 - height),
    width,
    height,
  };
}

/** The crop covers the whole picture: no crop. */
export function isWhole(crop: CropRect | null): boolean {
  return crop == null || (crop.x <= 0.0005 && crop.y <= 0.0005 && crop.width >= 0.999 && crop.height >= 0.999);
}
