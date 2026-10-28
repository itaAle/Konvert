import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { Info, Plus } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef } from "react";

import type { TargetFormat } from "../bindings/TargetFormat";
import { t } from "../i18n";
import { revealVariants } from "../lib/motion";
import {
  allWarnings,
  canCompress,
  canStart,
  combines,
  commonTargets,
  editedDocuments,
  editsPdf,
  lonePdf,
  pdfChanged,
  plannedItems,
  readyItems,
  totalEstimate,
} from "../state/queue";
import type { QueueController } from "../state/useQueue";
import { AdvancedPanel } from "./AdvancedPanel";
import { CompressCard } from "./CompressCard";
import { CompressPanel } from "./CompressPanel";
import { DestinationRow } from "./DestinationRow";
import { FileCard } from "./FileCard";
import { FileList } from "./FileList";
import { FormatGrid } from "./FormatGrid";
import { FramesPanel } from "./FramesPanel";
import { LocationRow } from "./LocationRow";
import { OcrPanel } from "./OcrPanel";
import { PageEditor, type EditorSource } from "./PageEditor";
import { PagesPanel } from "./PagesPanel";
import { PdfPanel } from "./PdfPanel";
import { PlanFooterInfo, PlanSummary, hasStrongWarning, isRemux } from "./PlanSummary";
import { ProtectPanel } from "./ProtectPanel";
import { StampPanel } from "./StampPanel";
import { QualityPicker } from "./QualityPicker";
import { SplitPanel } from "./SplitPanel";
import { Button } from "./ui/Button";
import { Page } from "./ui/Page";

/** Formats whose result does not depend on the quality preset. */
const FIXED_QUALITY: TargetFormat[] = ["bmp", "tiff", "stillGif", "ico"];

function SectionTitle({ children }: { children: string }) {
  return <h2 className="mb-3 px-1 text-[17px] font-semibold tracking-[-0.01em]">{children}</h2>;
}

/** Files, target format, quality, advanced options, summary and the primary action. */
export function SetupScreen({ q }: { q: QueueController }) {
  const { state } = q;
  const reduceMotion = useReducedMotion();
  const single = state.items.length === 1 && state.items[0].file != null;
  const targets = commonTargets(state.items);
  const ready = readyItems(state);
  const analyzing = state.items.some((i) => i.status === "analyzing");
  const option = targets.find((o) => o.format === state.target) ?? null;
  // Files left out of the PDF being made are not planned.
  const planned = plannedItems(state);
  const warnings = allWarnings(planned);
  const planError = planned.find((i) => i.planError)?.planError ?? null;
  const allPlanned = planned.length > 0 && planned.every((i) => i.plan);
  const estimate = allPlanned ? totalEstimate(planned) : null;
  const remux = allPlanned && planned.every((i) => isRemux(i.plan!));
  // Pictures from videos: the moment must exist in every video.
  const durations = ready.map((i) => i.file?.media.durationSecs ?? null);
  const shortest = durations.every((d) => d != null) && durations.length > 0 ? Math.min(...(durations as number[])) : null;
  const counts = ready.map((i) => i.plan?.frames?.count ?? null);
  const pictures = allPlanned && counts.every((n) => n != null) ? (counts as number[]).reduce((a, b) => a + b, 0) : null;
  const hasLocation = ready.some((i) => i.file?.media.hasLocation);
  // PDF documents: pictures of their pages, or PDFs merged together.
  const documents = ready.filter((i) => i.file?.media.kind === "document");
  const allDocuments = ready.length > 0 && documents.length === ready.length;
  // Paper, orientation and margins only matter for pictures made into pages.
  const photos = ready.filter((i) => i.file?.media.kind === "image");
  const pageCount = allDocuments ? Math.min(...documents.map((i) => i.file!.media.document?.pages ?? 1)) : 0;
  const pageCounts = ready.map((i) => i.plan?.document?.pages.length ?? null);
  const pagePictures = allPlanned && pageCounts.every((n) => n != null) ? (pageCounts as number[]).reduce((a, b) => a + b, 0) : null;
  const invalidPages = planError?.code === "invalid_pages";
  const together = combines(state);
  // A lone PDF made into new PDFs, or PDFs and photos combined: their pages
  // are edited, maybe split.
  const lone = lonePdf(state.items);
  const editing = editsPdf(state);
  const documentCount = editing ? editedDocuments(state) : null;
  const unchanged = editing && !together && !pdfChanged(state);
  const editorSources = useMemo<EditorSource[]>(
    () =>
      editing
        ? state.items
            .filter((i) => i.status === "ready" && i.file)
            .map((i) => {
              const media = i.file!.media;
              const photo = media.kind === "image" ? media.video[0] : null;
              return {
                key: i.key,
                path: i.path,
                name: i.name,
                count: media.kind === "document" ? (media.document?.pages ?? 1) : 1,
                picture: photo ? { width: photo.width ?? 1, height: photo.height ?? 1 } : null,
                firstPage: media.document ?? { widthPt: 595, heightPt: 842 },
              };
            })
        : [],
    [editing, state.items],
  );
  // Smaller copies, each file in its own format.
  const compressible = canCompress(state.items);
  const compressing = state.compress && compressible;
  const files = state.items.filter((i) => i.file && i.status !== "invalid");
  const formats = new Set(files.map((i) => i.file!.compress));
  const sharedFormat = formats.size === 1 ? (files[0]?.name.split(".").pop()?.toUpperCase() ?? null) : null;
  const largest = Math.max(0, ...files.map((i) => i.file!.media.sizeBytes ?? 0));

  // Bring quality and warnings into view once the options have appeared, and
  // whenever an important warning shows up, so it never stays below the fold.
  const summaryRef = useRef<HTMLDivElement>(null);
  const revealed = useRef(state.target != null);
  const scrollToSummary = useCallback(() => {
    summaryRef.current?.scrollIntoView({ block: "nearest", behavior: reduceMotion ? "auto" : "smooth" });
  }, [reduceMotion]);
  const strong = hasStrongWarning(warnings) || q.lowSpace != null;
  useEffect(() => {
    if (!strong || !revealed.current) return;
    const timer = setTimeout(scrollToSummary, 60);
    return () => clearTimeout(timer);
  }, [strong, warnings.length, scrollToSummary]);

  const footer = (
    <>
      <div className="min-w-0 flex-1">
        {state.target && ready.length > 0 && (
          <PlanFooterInfo estimate={estimate} total={ready.length > 1} remux={remux} loading={!allPlanned && !planError} />
        )}
      </div>
      <Button variant="primary" size="lg" disabled={!canStart(state)} onClick={() => void q.start()} className="min-w-44">
        {compressing
          ? ready.length > 1
            ? t("actions.compressAll", { n: ready.length })
            : t("actions.compress")
          : documentCount != null && documentCount > 1
          ? t("actions.splitPdf")
          : editing && !together
            ? t("actions.savePdf")
          : together || (state.target === "pdf" && ready.length === 1)
          ? t("actions.makePdf")
          : ready.length > 1
            ? t("actions.convertAll", { n: ready.length })
            : t("actions.convert")}
      </Button>
    </>
  );

  return (
    <Page footer={footer}>
      <div className="flex flex-col gap-6">
        {single ? (
          <div className="flex flex-col items-end gap-1">
            <div className="w-full">
              <FileCard file={state.items[0].file!} onRemove={q.reset} />
            </div>
            <Button variant="plain" size="sm" icon={<Plus size={15} aria-hidden />} onClick={() => void q.chooseFiles()}>
              {t("files.add")}
            </Button>
          </div>
        ) : (
          <FileList
            items={state.items}
            onRemove={q.remove}
            onAdd={() => void q.chooseFiles()}
            onClear={q.reset}
            onMove={together ? q.move : undefined}
            onUnlock={q.unlock}
          />
        )}

        <AnimatePresence initial={false}>
          {state.notice === "cancelled" && (
            <motion.p
              variants={revealVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              role="status"
              className="-mt-2 flex items-center gap-2 overflow-hidden px-1 text-[13px] text-label-secondary"
            >
              <Info size={15} aria-hidden />
              {t("cancelled")}
            </motion.p>
          )}
        </AnimatePresence>

        {compressible && (
          <section>
            <SectionTitle>{t("compress.title")}</SectionTitle>
            <CompressCard selected={compressing} onSelect={q.selectCompress} format={sharedFormat} />
          </section>
        )}

        {/* Files with nothing in common can still be compressed, each in its own format. */}
        {(targets.length > 0 || !compressible) && (
          <section>
            <SectionTitle>{t("formats.title")}</SectionTitle>
            {targets.length > 0 ? (
              <>
                {!single && <p className="-mt-2 mb-3 px-1 text-[13px] text-label-secondary">{t("formats.common")}</p>}
                <FormatGrid
                  options={targets}
                  selected={compressing ? null : state.target}
                  onSelect={q.selectTarget}
                  groupTitles={lone ? { document: "formats.groupEditPdf" } : undefined}
                  notes={lone ? { pdf: "formats.editPdf" } : undefined}
                />
              </>
            ) : (
              !analyzing && <p className="px-1 text-[14px] text-label-secondary">{t("formats.none")}</p>
            )}
          </section>
        )}

        <AnimatePresence initial={false}>
          {(option || compressing) && (
            <motion.div
              variants={revealVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              onAnimationComplete={(definition) => {
                if (definition !== "animate" || revealed.current) return;
                revealed.current = true;
                scrollToSummary();
              }}
              className="flex flex-col gap-6"
            >
              {compressing && (
                <section>
                  <SectionTitle>{t("compress.level")}</SectionTitle>
                  <CompressPanel
                    value={state.compression}
                    onChange={q.setCompression}
                    largestBytes={largest}
                    documents={documents.length > 0}
                  />
                </section>
              )}
              {option?.group === "frames" && (
                <section>
                  <SectionTitle>{t("frames.title")}</SectionTitle>
                  <FramesPanel value={state.frames} onChange={q.setFrames} duration={shortest} count={pictures} />
                </section>
              )}
              {option?.group === "pages" && (
                <section>
                  <SectionTitle>{t("pdf.pagesTitle")}</SectionTitle>
                  <PagesPanel
                    value={state.pdf}
                    onChange={q.setPdf}
                    pageCount={pageCount}
                    count={pagePictures}
                    invalid={invalidPages}
                  />
                </section>
              )}
              {option?.group === "document" && photos.length > 0 && (
                <section>
                  <SectionTitle>{t("pdf.pageTitle")}</SectionTitle>
                  <PdfPanel value={state.pdf} onChange={q.setPdf} canCombine={ready.length > 1 && documents.length === 0} />
                </section>
              )}
              {editing && (
                <>
                  <section>
                    <SectionTitle>{t("pdf.editTitle")}</SectionTitle>
                    <PageEditor
                      sources={editorSources}
                      pages={state.pages}
                      cuts={state.cuts}
                      split={state.pdf.split}
                      splitEvery={state.pdf.splitEvery}
                      watermark={state.pdf.watermark ?? null}
                      pageNumbers={state.pdf.pageNumbers ?? null}
                      onChange={q.setPages}
                    />
                  </section>
                  <section>
                    <SectionTitle>{t("pdf.resultTitle")}</SectionTitle>
                    <SplitPanel value={state.pdf} onChange={q.setPdf} documents={documentCount} unchanged={unchanged} />
                  </section>
                </>
              )}
              {option?.format === "pdf" && (
                <section>
                  <SectionTitle>{t("stamps.title")}</SectionTitle>
                  <StampPanel
                    watermark={state.pdf.watermark ?? null}
                    numbers={state.pdf.pageNumbers ?? null}
                    onWatermark={(watermark) => q.setPdf({ ...state.pdf, watermark })}
                    onNumbers={(pageNumbers) => q.setPdf({ ...state.pdf, pageNumbers })}
                    pickPicture={q.pickPicture}
                  />
                </section>
              )}
              {option?.format === "pdf" && (photos.length > 0 || documents.length > 0) && (
                <OcrPanel value={state.pdf.ocr} onChange={(ocr) => q.setPdf({ ...state.pdf, ocr })} />
              )}
              {option?.format === "pdf" && (
                <ProtectPanel value={state.pdf.protection ?? null} onChange={(protection) => q.setPdf({ ...state.pdf, protection })} />
              )}
              {option?.group === "document" && documents.length > 0 && !editing && (
                <p className="-mt-2 flex items-start gap-2 px-1 text-[13px] text-label-secondary">
                  <Info size={15} className="mt-0.5 shrink-0" aria-hidden />
                  {t("pdf.mergeHint")}
                </p>
              )}
              {/* Merged PDFs keep their pages as they are. */}
              {/* Merged PDFs and Word documents keep their pages as they are. */}
              {option && !FIXED_QUALITY.includes(option.format) && !(option.group === "document" && photos.length === 0) && option.group !== "text" && (
                <section>
                  <SectionTitle>{t("quality.title")}</SectionTitle>
                  <QualityPicker value={state.preset} onChange={q.selectPreset} />
                </section>
              )}
              {option && <AdvancedPanel option={option} value={state.advanced} onChange={q.setAdvanced} />}
              {/* Frames, PDF pages and text never keep the location. */}
              {hasLocation &&
                (compressing || (option?.group !== "frames" && option?.group !== "document" && option?.group !== "text")) && (
                <LocationRow checked={state.removeLocation} onChange={q.setRemoveLocation} />
              )}
              {/* Margin keeps the content clear of the faded edge above the footer. */}
              <div ref={summaryRef} className="flex scroll-mb-10 flex-col gap-4">
                <PlanSummary warnings={warnings} error={invalidPages && !editing ? null : planError} lowSpace={q.lowSpace} />
                <DestinationRow />
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </Page>
  );
}
