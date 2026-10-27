import type { PdfOptions } from "../bindings/PdfOptions";
import { intlLocale, t } from "../i18n";
import { Segmented } from "./ui/Segmented";
import { Select } from "./ui/Select";

/** Pages per document offered when splitting every few pages. */
const EVERY = [1, 2, 3, 4, 5, 10, 20];
const AT_CUTS = "cuts";

interface SplitPanelProps {
  value: PdfOptions;
  onChange: (value: PdfOptions) => void;
  /** Documents the plan makes (`null`: not planned yet). */
  documents: number | null;
  /** Nothing is changed yet: there is nothing to save. */
  unchanged: boolean;
}

/** Edited PDF: one document, or split every few pages or where the user chooses. */
export function SplitPanel({ value, onChange, documents, unchanged }: SplitPanelProps) {
  const splitting = value.split !== "none";
  const by = value.split === "after" ? AT_CUTS : String(value.splitEvery);
  const number = (n: number) => new Intl.NumberFormat(intlLocale()).format(n);

  const hint = !splitting
    ? unchanged
      ? t("pdf.unchanged")
      : t("pdf.oneHint")
    : documents != null && documents > 1
      ? t("pdf.splitCount", { n: number(documents) })
      : value.split === "after"
        ? t("pdf.cutsHint")
        : t("pdf.splitNone");

  return (
    <div className="flex flex-col gap-3">
      <Segmented<"one" | "split">
        label={t("pdf.resultTitle")}
        value={splitting ? "split" : "one"}
        options={[
          { value: "one", label: t("pdf.one") },
          { value: "split", label: t("pdf.split") },
        ]}
        onChange={(mode) => onChange({ ...value, split: mode === "one" ? "none" : "every" })}
      />
      {splitting && (
        <div className="grid grid-cols-2 gap-3 px-1">
          <Select
            label={t("pdf.splitBy")}
            value={by}
            options={[
              ...EVERY.map((n) => ({ value: String(n), label: n === 1 ? t("pdf.everyPage") : t("pdf.everyPages", { n }) })),
              { value: AT_CUTS, label: t("pdf.atCuts") },
            ]}
            onChange={(v) =>
              onChange(v === AT_CUTS ? { ...value, split: "after" } : { ...value, split: "every", splitEvery: Number(v) })
            }
          />
        </div>
      )}
      <p className="px-1 text-[13px] text-label-secondary" aria-live="polite">
        {value.split === "after" && documents != null && documents > 1 ? `${t("pdf.splitCount", { n: number(documents) })} ${t("pdf.cutsHint")}` : hint}
      </p>
    </div>
  );
}
