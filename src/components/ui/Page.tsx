import type { CSSProperties, ReactNode } from "react";

interface PageProps {
  children: ReactNode;
  /** Fixed bottom area (primary action); the content scrolls above it. */
  footer?: ReactNode;
  /** Vertically center the content. */
  center?: boolean;
}

// Content fades out under the footer. A mask (not a painted gradient) also
// works over the translucent Mica backdrop.
const fadeBottom: CSSProperties = {
  maskImage: "linear-gradient(to bottom, black calc(100% - 32px), transparent)",
};

/** Full-height screen: scrollable centered column plus an optional footer. */
export function Page({ children, footer, center = false }: PageProps) {
  return (
    <>
      <div className="min-h-0 flex-1 overflow-y-auto" style={footer ? fadeBottom : undefined}>
        <div
          className={`mx-auto flex min-h-full w-full max-w-[680px] flex-col px-8 pt-8 ${
            footer ? "pb-8" : "pb-10"
          } ${center ? "justify-center" : ""}`}
        >
          {children}
        </div>
      </div>
      {footer && (
        <div className="shrink-0">
          <div className="mx-auto flex w-full max-w-[680px] items-center justify-end gap-3 px-8 pt-2 pb-7">
            {footer}
          </div>
        </div>
      )}
    </>
  );
}
