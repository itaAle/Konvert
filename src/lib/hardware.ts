// Names of the hardware video encoders (brands and codecs: the same in every language).
import type { Accelerator } from "../bindings/Accelerator";
import type { ConversionPlan } from "../bindings/ConversionPlan";
import type { HardwareEncoder } from "../bindings/HardwareEncoder";
import type { VideoCodec } from "../bindings/VideoCodec";

const ACCELERATORS: Record<Accelerator, string> = {
  nvenc: "NVIDIA NVENC",
  qsv: "Intel Quick Sync",
  amf: "AMD AMF",
  vaapi: "VA-API",
  videoToolbox: "Apple VideoToolbox",
};

const CODECS: Partial<Record<VideoCodec, string>> = { h264: "H.264", av1: "AV1" };

export function acceleratorName(accelerator: Accelerator): string {
  return ACCELERATORS[accelerator];
}

/** "NVIDIA NVENC: H.264, AV1", one entry per vendor, in the order found. */
export function hardwareSummary(encoders: HardwareEncoder[]): string[] {
  const byVendor = new Map<Accelerator, string[]>();
  for (const encoder of encoders) {
    const codecs = byVendor.get(encoder.accelerator) ?? [];
    codecs.push(CODECS[encoder.codec] ?? encoder.codec);
    byVendor.set(encoder.accelerator, codecs);
  }
  return [...byVendor].map(([accelerator, codecs]) => `${acceleratorName(accelerator)}: ${codecs.join(", ")}`);
}

/** The hardware encoder of a plan's video, if the graphics chip encodes it. */
export function planHardware(plan: ConversionPlan): HardwareEncoder | null {
  return plan.video?.mode === "encode" ? (plan.video.hardware ?? null) : null;
}
