import { Eye, EyeOff, LockKeyhole } from "lucide-react";
import { useEffect, useId, useState } from "react";

import type { PdfProtection } from "../bindings/PdfProtection";
import { t } from "../i18n";
import { Switch } from "./ui/Switch";

interface ProtectPanelProps {
  value: PdfProtection | null;
  /**
   * `null`: no password. While the two passwords are empty or differ, the
   * protection has an empty password (nothing can start).
   */
  onChange: (value: PdfProtection | null) => void;
}

/**
 * Protects the new PDF with a password typed twice. The password stays in
 * this form and in the request while the window is open: it is never saved.
 */
export function ProtectPanel({ value, onChange }: ProtectPanelProps) {
  const [password, setPassword] = useState("");
  const [again, setAgain] = useState("");
  const [shown, setShown] = useState(false);
  const hintId = useId();
  const passwordId = useId();
  const againId = useId();
  const on = value != null;
  const allowPrint = value?.allowPrint ?? true;
  const allowCopy = value?.allowCopy ?? true;
  const mismatch = again !== "" && again !== password;

  // Shown again (another format was chosen in between): the fields are empty,
  // so the password must be typed again.
  useEffect(() => {
    if (value && value.password !== "") onChange({ ...value, password: "" });
    // Only when the panel appears.
  }, []);

  const send = (next: { password?: string; again?: string; allowPrint?: boolean; allowCopy?: boolean }) => {
    const p = next.password ?? password;
    const a = next.again ?? again;
    onChange({
      password: p !== "" && p === a ? p : "",
      allowPrint: next.allowPrint ?? allowPrint,
      allowCopy: next.allowCopy ?? allowCopy,
    });
  };
  const toggle = (checked: boolean) => {
    setPassword("");
    setAgain("");
    onChange(checked ? { password: "", allowPrint: true, allowCopy: true } : null);
  };

  const field = "h-9 w-full rounded-[10px] bg-fill px-3 text-[14px] text-label transition-colors hover:bg-fill-strong focus:bg-fill-strong";
  return (
    <div className="flex flex-col gap-3 rounded-[14px] bg-surface px-4 py-3 shadow-card">
      <div className="flex items-start gap-3">
        <LockKeyhole size={17} className="mt-0.5 shrink-0 text-label-secondary" aria-hidden />
        <div className="min-w-0 flex-1">
          <p className="text-[14px] font-medium">{t("protect.title")}</p>
          <p id={hintId} className="mt-0.5 text-[12px] leading-snug text-label-secondary">
            {t("protect.hint")}
          </p>
        </div>
        <Switch checked={on} onChange={toggle} label={t("protect.title")} describedBy={hintId} />
      </div>
      {on && (
        <div className="flex flex-col gap-3 pl-8">
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1">
              <label htmlFor={passwordId} className="px-1 text-[12px] font-medium text-label-secondary">
                {t("protect.password")}
              </label>
              <div className="relative">
                <input
                  id={passwordId}
                  type={shown ? "text" : "password"}
                  autoComplete="new-password"
                  spellCheck={false}
                  value={password}
                  onChange={(e) => {
                    setPassword(e.target.value);
                    send({ password: e.target.value });
                  }}
                  className={`${field} pr-9`}
                />
                <button
                  type="button"
                  onClick={() => setShown((v) => !v)}
                  aria-label={shown ? t("unlock.hide") : t("unlock.show")}
                  title={shown ? t("unlock.hide") : t("unlock.show")}
                  className="absolute top-1/2 right-1 flex size-7 -translate-y-1/2 items-center justify-center rounded-full text-label-tertiary hover:text-label"
                >
                  {shown ? <EyeOff size={15} aria-hidden /> : <Eye size={15} aria-hidden />}
                </button>
              </div>
            </div>
            <div className="flex flex-col gap-1">
              <label htmlFor={againId} className="px-1 text-[12px] font-medium text-label-secondary">
                {t("protect.again")}
              </label>
              <input
                id={againId}
                type={shown ? "text" : "password"}
                autoComplete="new-password"
                spellCheck={false}
                value={again}
                aria-invalid={mismatch}
                onChange={(e) => {
                  setAgain(e.target.value);
                  send({ again: e.target.value });
                }}
                className={`${field} ${mismatch ? "ring-2 ring-warning" : ""}`}
              />
            </div>
          </div>
          {mismatch && (
            <p role="alert" className="-mt-1 px-1 text-[12px] text-warning">
              {t("protect.mismatch")}
            </p>
          )}
          <div className="flex items-center gap-3">
            <p className="min-w-0 flex-1 text-[13px]">{t("protect.allowPrint")}</p>
            <Switch checked={allowPrint} onChange={(v) => send({ allowPrint: v })} label={t("protect.allowPrint")} />
          </div>
          <div className="flex items-center gap-3">
            <p className="min-w-0 flex-1 text-[13px]">{t("protect.allowCopy")}</p>
            <Switch checked={allowCopy} onChange={(v) => send({ allowCopy: v })} label={t("protect.allowCopy")} />
          </div>
          <p className="text-[12px] leading-snug text-label-secondary">
            {allowPrint && allowCopy ? t("protect.forget") : t("protect.limits")}
          </p>
        </div>
      )}
    </div>
  );
}
