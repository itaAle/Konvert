import { afterEach, describe, expect, it } from "vitest";
import { allKeys, dictionaries, errorMessage, setLocale, systemLocale, t } from ".";

const placeholders = (s: string) => (s.match(/\{\w+\}/g) ?? []).sort();

function lookup(dict: object, key: string): string {
  return key.split(".").reduce((node: any, k) => node[k], dict) as string;
}

describe("i18n", () => {
  afterEach(() => setLocale("it"));

  it("every language has every key, non-empty, with the same placeholders", () => {
    const keys = allKeys(dictionaries.it);
    for (const [locale, dict] of Object.entries(dictionaries)) {
      expect(allKeys(dict).sort(), locale).toEqual([...keys].sort());
      for (const key of keys) {
        const value = lookup(dict, key);
        expect(value.trim().length, `${locale}.${key}`).toBeGreaterThan(0);
        expect(placeholders(value), `${locale}.${key}`).toEqual(placeholders(lookup(dictionaries.it, key)));
      }
    }
  });

  it("translates and fills placeholders", () => {
    expect(t("done.saved", { name: "brano.mp3" })).toBe("Salvato come brano.mp3");
    setLocale("en");
    expect(t("done.saved", { name: "song.mp3" })).toBe("Saved as song.mp3");
  });

  it("maps backend error codes, with a fallback", () => {
    expect(errorMessage("disk_full")).toContain("Spazio su disco");
    expect(errorMessage("something_new")).toBe(t("errors.unknown"));
  });

  it("picks Italian only for Italian systems", () => {
    expect(systemLocale(["it-IT", "en-US"])).toBe("it");
    expect(systemLocale(["de-DE", "it"])).toBe("it");
    expect(systemLocale(["en-US"])).toBe("en");
    expect(systemLocale(["fr-FR"])).toBe("en");
  });

  it("keeps the required privacy sentence", () => {
    expect(dictionaries.it.about.privacy).toBe(
      "Konvert funziona interamente sul tuo computer. Nessun dato lascia il dispositivo.",
    );
  });
});
