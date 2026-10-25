// Minimal typed i18n: dot-separated keys checked by the compiler, `{param}`
// placeholders. To add a language: create a dictionary of type `Messages`,
// register it in `dictionaries` and in `LanguagePref` (Rust preferences).
import { en } from "./en";
import { it, type Messages } from "./it";
import type { LanguagePref } from "../bindings/LanguagePref";

export type Locale = "it" | "en";

const dictionaries: Record<Locale, Messages> = { it, en };
const intlTags: Record<Locale, string> = { it: "it-IT", en: "en-GB" };

/** All dot-separated paths to string leaves, e.g. "drop.title". */
type Paths<T, Prefix extends string = ""> = {
  [K in keyof T & string]: T[K] extends string
    ? `${Prefix}${K}`
    : Paths<T[K], `${Prefix}${K}.`>;
}[keyof T & string];

export type MessageKey = Paths<Messages>;

let current: Messages = dictionaries.it;
let currentLocale: Locale = "it";

/** System language → supported locale (Italian for Italian systems, English otherwise). */
export function systemLocale(
  languages: readonly string[] = typeof navigator === "undefined" ? [] : (navigator.languages ?? [navigator.language]),
): Locale {
  return languages.some((l) => l.toLowerCase().startsWith("it")) ? "it" : "en";
}

export function resolveLocale(pref: LanguagePref): Locale {
  return pref === "auto" ? systemLocale() : pref;
}

export function setLocale(locale: Locale): void {
  current = dictionaries[locale];
  currentLocale = locale;
  if (typeof document !== "undefined") document.documentElement.lang = locale;
}

export function getLocale(): Locale {
  return currentLocale;
}

/** BCP 47 tag for `Intl` formatters. */
export function intlLocale(): string {
  return intlTags[currentLocale];
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

/** For tests: every leaf key of a dictionary. */
export function allKeys(dict: Messages = it): string[] {
  const walk = (node: object, prefix: string): string[] =>
    Object.entries(node).flatMap(([k, v]) =>
      typeof v === "string" ? [`${prefix}${k}`] : walk(v as object, `${prefix}${k}.`),
    );
  return walk(dict, "");
}

export { dictionaries };
