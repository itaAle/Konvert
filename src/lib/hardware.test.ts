import { describe, expect, it } from "vitest";

import type { ConversionPlan } from "../bindings/ConversionPlan";
import { acceleratorName, hardwareSummary, planHardware } from "./hardware";

describe("hardware encoders", () => {
  it("names every vendor", () => {
    expect(acceleratorName("nvenc")).toBe("NVIDIA NVENC");
    expect(acceleratorName("videoToolbox")).toBe("Apple VideoToolbox");
  });

  it("groups the codecs of each vendor, in the order found", () => {
    expect(
      hardwareSummary([
        { accelerator: "nvenc", codec: "h264" },
        { accelerator: "qsv", codec: "h264" },
        { accelerator: "nvenc", codec: "av1" },
      ]),
    ).toEqual(["NVIDIA NVENC: H.264, AV1", "Intel Quick Sync: H.264"]);
    expect(hardwareSummary([])).toEqual([]);
  });

  it("finds the encoder of a plan's video", () => {
    const base = { video: null } as unknown as ConversionPlan;
    expect(planHardware(base)).toBeNull();
    expect(planHardware({ ...base, video: { mode: "copy", codec: "h264" } })).toBeNull();
    const encoded = {
      mode: "encode",
      codec: "h264",
      rate: { kind: "bitrate", kbps: 4000 },
      width: 1920,
      height: 1080,
      resize: false,
      fps: null,
      alpha: false,
    } as const;
    expect(planHardware({ ...base, video: encoded })).toBeNull();
    expect(planHardware({ ...base, video: { ...encoded, hardware: { accelerator: "qsv", codec: "h264" } } })).toEqual({
      accelerator: "qsv",
      codec: "h264",
    });
  });
});
