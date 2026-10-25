import { ChevronsUpDown } from "lucide-react";
import { useId } from "react";

export interface SelectOption<T extends string> {
  value: T;
  label: string;
}

interface SelectProps<T extends string> {
  label: string;
  value: T;
  options: SelectOption<T>[];
  onChange: (value: T) => void;
  disabled?: boolean;
}

/** macOS-style popup button (a native, fully accessible <select>). */
export function Select<T extends string>({ label, value, options, onChange, disabled }: SelectProps<T>) {
  const id = useId();
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <label htmlFor={id} className="px-1 text-[12px] font-medium text-label-secondary">
        {label}
      </label>
      <div className="relative">
        <select
          id={id}
          value={value}
          disabled={disabled}
          onChange={(e) => onChange(e.target.value as T)}
          className="h-9 w-full appearance-none truncate rounded-[10px] bg-fill pr-8 pl-3 text-[14px] text-label transition-colors hover:bg-fill-strong disabled:opacity-50"
        >
          {options.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
        <ChevronsUpDown
          size={14}
          aria-hidden
          className="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-label-tertiary"
        />
      </div>
    </div>
  );
}
