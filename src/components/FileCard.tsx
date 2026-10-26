import { X } from "lucide-react";

import type { FileAnalysis } from "../bindings/FileAnalysis";
import { t } from "../i18n";
import { describeMedia, formatBytes, formatDuration } from "../lib/format";
import { kindStyle } from "../lib/kind";

interface FileCardProps {
  file: FileAnalysis;
  onRemove?: () => void;
}

/** Icon, name, duration, size and a one-line technical summary of the file. */
export function FileCard({ file, onRemove }: FileCardProps) {
  const { media } = file;
  const { Icon, gradient } = kindStyle(media.kind);
  const facts = [
    media.durationSecs != null ? formatDuration(media.durationSecs) : null,
    media.sizeBytes != null ? formatBytes(media.sizeBytes) : null,
  ].filter(Boolean);

  return (
    <section
      aria-label={file.name}
      className="flex items-center gap-4 rounded-[22px] bg-surface p-4 pr-3 shadow-card"
    >
      <div
        className={`flex size-14 shrink-0 items-center justify-center rounded-[16px] bg-gradient-to-b text-white ${gradient}`}
      >
        <Icon size={26} strokeWidth={1.9} aria-hidden />
      </div>
      <div className="min-w-0 flex-1">
        <p className="truncate text-[16px] font-semibold tracking-[-0.01em]" title={file.name}>
          {file.name}
        </p>
        <p className="mt-0.5 truncate text-[13px] text-label-secondary">
          {[...facts, describeMedia(media)].join(" · ")}
        </p>
      </div>
      {onRemove && (
        <button
          type="button"
          onClick={onRemove}
          aria-label={t("file.change")}
          title={t("file.change")}
          className="flex size-8 shrink-0 items-center justify-center rounded-full text-label-secondary transition-colors hover:bg-fill hover:text-label"
        >
          <X size={18} aria-hidden />
        </button>
      )}
    </section>
  );
}
