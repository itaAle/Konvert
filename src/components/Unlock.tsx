import { Eye, EyeOff, LockKeyhole } from "lucide-react";
import { useId, useState, type FormEvent } from "react";

import type { ErrorPayload } from "../bindings/ErrorPayload";
import { errorMessage, t } from "../i18n";
import { Button } from "./ui/Button";

/** Tries a password; gives the error when it does not open the file. */
export type UnlockAttempt = (password: string) => Promise<ErrorPayload | null>;

/**
 * Password field of a PDF that needs one. The password goes to the backend,
 * which keeps it in memory while the file is in the list; the field is
 * emptied after every attempt.
 */
export function UnlockForm({ name, onUnlock, compact = false }: { name: string; onUnlock: UnlockAttempt; compact?: boolean }) {
  const [password, setPassword] = useState("");
  const [shown, setShown] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ErrorPayload | null>(null);
  const fieldId = useId();
  const errorId = useId();

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (password === "" || busy) return;
    setBusy(true);
    const failed = await onUnlock(password);
    setPassword("");
    setBusy(false);
    setError(failed);
  };

  return (
    <form onSubmit={(e) => void submit(e)} className={`flex flex-col gap-1.5 ${compact ? "" : "w-full max-w-sm"}`}>
      <label htmlFor={fieldId} className={compact ? "sr-only" : "px-1 text-[12px] font-medium text-label-secondary"}>
        {t("unlock.password", { name })}
      </label>
      <div className="flex items-center gap-2">
        <div className="relative min-w-0 flex-1">
          <input
            id={fieldId}
            type={shown ? "text" : "password"}
            autoComplete="off"
            spellCheck={false}
            autoFocus={!compact}
            value={password}
            placeholder={compact ? t("unlock.placeholder") : undefined}
            aria-invalid={error != null}
            aria-describedby={error ? errorId : undefined}
            onChange={(e) => {
              setPassword(e.target.value);
              setError(null);
            }}
            className={`w-full rounded-[10px] bg-fill pr-9 pl-3 text-label transition-colors placeholder:text-label-tertiary hover:bg-fill-strong focus:bg-fill-strong ${
              compact ? "h-8 text-[13px]" : "h-10 text-[15px]"
            } ${error ? "ring-2 ring-warning" : ""}`}
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
        <Button type="submit" variant={compact ? "secondary" : "primary"} size={compact ? "sm" : "md"} disabled={password === "" || busy}>
          {t("unlock.open")}
        </Button>
      </div>
      {error && (
        <p id={errorId} role="alert" className="px-1 text-[12px] text-warning">
          {errorMessage(error.code)}
        </p>
      )}
    </form>
  );
}

/** A single PDF that needs a password: ask for it. */
export function UnlockScreen({ name, onUnlock, onChooseAnother }: { name: string; onUnlock: UnlockAttempt; onChooseAnother: () => void }) {
  return (
    <div className="flex flex-col items-center justify-center text-center">
      <div className="flex size-20 items-center justify-center rounded-full bg-accent-soft text-accent">
        <LockKeyhole size={34} strokeWidth={1.8} aria-hidden />
      </div>
      <h1 className="mt-6 text-[24px] font-semibold tracking-[-0.02em]">{t("unlock.title")}</h1>
      <p className="mt-2 max-w-md text-[15px] leading-relaxed text-label-secondary">{t("unlock.hint", { name })}</p>
      <div className="mt-7 flex w-full justify-center">
        <UnlockForm name={name} onUnlock={onUnlock} />
      </div>
      <p className="mt-4 max-w-md text-[12px] leading-snug text-label-tertiary">{t("unlock.privacy")}</p>
      <div className="mt-8">
        <Button variant="secondary" onClick={onChooseAnother}>
          {t("actions.chooseAnother")}
        </Button>
      </div>
    </div>
  );
}
