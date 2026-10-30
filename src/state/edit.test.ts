import { describe, expect, it } from "vitest";

import {
  DEFAULT_EDIT,
  clampCrop,
  cropWithRatio,
  editChanged,
  editedSize,
  flipCrop,
  isWhole,
  pictureEditOf,
  ratioValue,
  turnCrop,
  type EditScope,
} from "./edit";

const ALL: EditScope = { turn: true, crop: true, resize: true };
const photo = { width: 4032, height: 3024 };

describe("picture edits", () => {
  it("sizes follow the core's rules", () => {
    const size = (patch: Partial<typeof DEFAULT_EDIT>) => editedSize(photo, { ...DEFAULT_EDIT, ...patch }, ALL);
    expect(size({})).toMatchObject({ width: 4032, height: 3024, stretched: false });
    expect(size({ resize: "percent", percent: 50 })).toMatchObject({ width: 2016, height: 1512 });
    // Not larger unless asked.
    expect(size({ resize: "percent", percent: 200 })).toMatchObject({ width: 4032, height: 3024 });
    expect(size({ resize: "percent", percent: 200, enlarge: true })).toMatchObject({ width: 8064, height: 6048 });
    expect(size({ resize: "longSide", side: 1920 })).toMatchObject({ width: 1920, height: 1440 });
    expect(size({ resize: "pixels", width: 1000 })).toMatchObject({ width: 1000, height: 750 });
    expect(size({ resize: "pixels", width: 1000, height: 1000 })).toMatchObject({ width: 1000, height: 750 });
    expect(size({ resize: "pixels", width: 1000, height: 1000, keepRatio: false })).toMatchObject({
      width: 1000,
      height: 1000,
      stretched: true,
    });
    // Turned a quarter, the top half kept, then halved.
    expect(
      size({ rotation: 90, crop: { x: 0, y: 0, width: 1, height: 0.5 }, resize: "percent", percent: 50 }),
    ).toMatchObject({ width: 1512, height: 1008 });
    // Pictures from videos are not turned or cropped.
    expect(editedSize(photo, { ...DEFAULT_EDIT, rotation: 90 }, { turn: false, crop: false, resize: true })).toMatchObject(
      photo,
    );
  });

  it("only real changes are sent", () => {
    expect(pictureEditOf(DEFAULT_EDIT, ALL, null)).toBeNull();
    expect(editChanged(DEFAULT_EDIT, ALL)).toBe(false);
    // An empty percentage changes nothing.
    expect(editChanged({ ...DEFAULT_EDIT, resize: "percent", percent: null }, ALL)).toBe(false);
    expect(pictureEditOf({ ...DEFAULT_EDIT, rotation: 90 }, ALL, null)).toMatchObject({ rotation: 90, copy: false });
    // A copy is always an edit, named after its label.
    expect(pictureEditOf(DEFAULT_EDIT, ALL, "mod")).toMatchObject({ copy: true, label: "mod" });
    // A crop is left out where it does not apply (several pictures).
    const cropped = { ...DEFAULT_EDIT, crop: { x: 0.1, y: 0.1, width: 0.5, height: 0.5 } };
    expect(pictureEditOf(cropped, { ...ALL, crop: false }, null)).toBeNull();
    expect(pictureEditOf({ ...DEFAULT_EDIT, resize: "pixels", width: 800, keepRatio: false }, ALL, null)?.resize).toEqual({
      mode: "pixels",
      width: 800,
      height: null,
      keepRatio: false,
    });
  });

  it("crops follow the picture when it turns or mirrors", () => {
    const crop = { x: 0.1, y: 0.2, width: 0.3, height: 0.4 };
    const right = turnCrop(crop, true)!;
    expect(right.x).toBeCloseTo(0.4);
    expect(right).toMatchObject({ y: 0.1, width: 0.4, height: 0.3 });
    // Four quarter turns, or a turn back, give the same crop.
    const back = turnCrop(right, false)!;
    expect(back.x).toBeCloseTo(crop.x);
    expect(back.y).toBeCloseTo(crop.y);
    let round = crop;
    for (let i = 0; i < 4; i++) round = turnCrop(round, true)!;
    expect(round.x).toBeCloseTo(crop.x);
    expect(round.height).toBeCloseTo(crop.height);
    expect(flipCrop(crop, true)!.x).toBeCloseTo(0.6);
    expect(flipCrop(crop, false)!.y).toBeCloseTo(0.4);
    expect(turnCrop(null, true)).toBeNull();
  });

  it("fixed proportions give the largest centered crop", () => {
    const square = cropWithRatio(ratioValue("1:1", false, photo)!, photo);
    expect(square.height).toBe(1);
    expect(square.width).toBeCloseTo(0.75);
    expect(square.x).toBeCloseTo(0.125);
    // 16:9 portrait on a landscape photo: as high as the photo.
    const tall = cropWithRatio(ratioValue("16:9", true, photo)!, photo);
    expect(tall.height).toBe(1);
    expect(tall.width * photo.width).toBeCloseTo((photo.height * 9) / 16);
    // Around an existing crop, kept inside the picture.
    const near = cropWithRatio(1, photo, { x: 0.9, y: 0, width: 0.1, height: 0.1 });
    expect(near.x + near.width).toBeCloseTo(1);
    expect(ratioValue("free", false, photo)).toBeNull();
    expect(ratioValue("original", false, photo)).toBeCloseTo(4 / 3);
  });

  it("crops stay inside the picture", () => {
    expect(clampCrop({ x: 0.9, y: -0.2, width: 0.5, height: 0.001 })).toEqual({ x: 0.5, y: 0, width: 0.5, height: 0.02 });
    expect(isWhole({ x: 0, y: 0, width: 1, height: 1 })).toBe(true);
    expect(isWhole({ x: 0.1, y: 0, width: 0.9, height: 1 })).toBe(false);
  });
});
