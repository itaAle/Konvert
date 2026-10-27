// Icon and tile color of each kind of file (audio, video, picture, document).
import { Clapperboard, FileText, Image as ImageIcon, Music2, type LucideIcon } from "lucide-react";

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
    case "document":
      return { Icon: FileText, gradient: "from-[#ff9f0a] to-[#e67700]" };
    case "text":
      return { Icon: FileText, gradient: "from-[#409cff] to-[#0a5fd6]" };
    default:
      return { Icon: Clapperboard, gradient: "from-[#bf5af2] to-[#8e44d6]" };
  }
}
