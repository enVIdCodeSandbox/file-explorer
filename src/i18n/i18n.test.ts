import { describe, expect, it } from "vitest";
import { KEYS } from "./keys";
import { LANGUAGES, resolveLanguage, translate } from "./index";
import de from "./locales/de";
import es from "./locales/es";
import fr from "./locales/fr";
import it_ from "./locales/it";
import ja from "./locales/ja";
import ko from "./locales/ko";
import pt from "./locales/pt";
import ru from "./locales/ru";
import zh from "./locales/zh";

const all: Record<string, Record<string, string>> = { de, es, fr, it: it_, ja, ko, pt, ru, zh };

describe("languages", () => {
  it("follows the system language and falls back to English", () => {
    expect(resolveLanguage("auto", ["fr-CA", "en-US"])).toBe("fr");
    expect(resolveLanguage("auto", ["sv-SE", "de-AT"])).toBe("de");
    expect(resolveLanguage("auto", ["sv-SE"])).toBe("en");
    expect(resolveLanguage(undefined, ["ja"])).toBe("ja");
  });
  it("a saved choice wins over the system", () => {
    expect(resolveLanguage("es", ["fr"])).toBe("es");
    expect(resolveLanguage("xx", ["fr"])).toBe("en");
  });
  it("uses English where a string has no translation, and fills placeholders", () => {
    expect(translate("zh", "No such string")).toBe("No such string");
    expect(translate("en", "Last {n}", { n: 5 })).toBe("Last 5");
    expect(translate("de", "Last {n}", { n: 5 })).toContain("5");
  });
  it("has a dictionary for every listed language but English", () => {
    for (const l of LANGUAGES) if (l.code !== "en") expect(Object.keys(all[l.code]).length).toBeGreaterThan(50);
  });
  it("keeps every placeholder in every translation", () => {
    for (const [code, d] of Object.entries(all)) {
      expect(Object.keys(d).length, code).toBe(KEYS.length);
      for (const k of KEYS) {
        const want = (k.match(/\{\w+\}/g) ?? []).sort().join();
        const got = (d[k].match(/\{\w+\}/g) ?? []).sort().join();
        expect(got, `${code}: ${k}`).toBe(want);
        expect(d[k].trim().length, `${code}: ${k}`).toBeGreaterThan(0);
      }
    }
  });
});
