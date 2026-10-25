// Minimal typed i18n: dot-separated keys checked by the compiler, `{param}`
// placeholders. Italian is the default; more languages are added by creating a
// dictionary of type `Messages` and registering it in `dictionaries`.
import { it, type Messages } from "./it";

export type Locale = "it";

const dictionaries: Record<Locale, Messages> = { it };

/** All dot-separated paths to string leaves, e.g. "drop.title". */
type Paths<T, Prefix extends string = ""> = {
  [K in keyof T & string]: T[K] extends string
    ? `${Prefix}${K}`
    : Paths<T[K], `${Prefix}${K}.`>;
}[keyof T & string];

export type MessageKey = Paths<Messages>;

let current: Messages = dictionaries.it;
let currentLocale: Locale = "it";

export function setLocale(locale: Locale): void {
  current = dictionaries[locale];
  currentLocale = locale;
}

export function getLocale(): Locale {
  return currentLocale;
}

/** BCP 47 tag for `Intl` formatters. */
export function intlLocale(): string {
  return { it: "it-IT" }[currentLocale];
}

function lookup(dict: Messages, key: string): string | undefined {
  let node: unknown = dict;
  for (const part of key.split(".")) {
    if (typeof node !== "object" || node === null) return undefined;
    node = (node as Record<string, unknown>)[part];
  }
  return typeof node === "string" ? node : undefined;
}

/** Translates `key`, replacing `{name}` placeholders with `params`. */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const template = lookup(current, key) ?? key;
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(params[name]) : match,
  );
}

/** Translation for a backend error code, with a generic fallback. */
export function errorMessage(code: string): string {
  return lookup(current, `errors.${code}`) ?? t("errors.unknown");
}
