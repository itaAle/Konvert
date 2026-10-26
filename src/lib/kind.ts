// Icon and tile color of each kind of file (audio, video, picture).
import { Clapperboard, Image as ImageIcon, Music2, type LucideIcon } from "lucide-react";

import type { MediaKind } from "../bindings/MediaKind";

export interface KindStyle {
  Icon: LucideIcon;
  /** Tailwind gradient classes of the icon tile. */
  gradient: string;
}

export function kindStyle(kind: MediaKind | undefined): KindStyle {
  switch (kind) {
    case "audio":
      return { Icon: Music2, gradient: "from-[#ff6482] to-[#e8364f]" };
    case "image":
      return { Icon: ImageIcon, gradient: "from-[#40c8e0] to-[#1a9bb5]" };
    default:
      return { Icon: Clapperboard, gradient: "from-[#bf5af2] to-[#8e44d6]" };
  }
}
