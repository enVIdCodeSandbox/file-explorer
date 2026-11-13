// Interface languages. The English text itself is the key: `t("Next")` returns
// the translation when the chosen language has one and the English text when it
// does not, so a screen can be moved over a piece at a time and nothing ever
// shows an empty or technical label. Add a language by adding a file in
// ./locales and one line to LANGUAGES.

import { useStore } from "../store";
import { adv } from "../settings/advanced";
import de from "./locales/de";
import es from "./locales/es";
import fr from "./locales/fr";
import it from "./locales/it";
import ja from "./locales/ja";
import ko from "./locales/ko";
import pt from "./locales/pt";
import ru from "./locales/ru";
import zh from "./locales/zh";

export type Dict = Record<string, string>;

/** Each language is shown by its own name so a person can find theirs whatever the app is set to now. */
export const LANGUAGES: { code: string; name: string }[] = [
  { code: "en", name: "English" },
  { code: "es", name: "Español" },
  { code: "fr", name: "Français" },
  { code: "de", name: "Deutsch" },
  { code: "pt", name: "Português" },
  { code: "it", name: "Italiano" },
  { code: "ru", name: "Русский" },
  { code: "zh", name: "简体中文" },
  { code: "ja", name: "日本語" },
  { code: "ko", name: "한국어" },
];

const DICTS: Record<string, Dict> = { de, es, fr, it, ja, ko, pt, ru, zh };

/** The language to use for a saved choice: "auto" follows the system, falling back to English. */
export function resolveLanguage(pref: string | undefined, system: readonly string[] = navigator.languages ?? [navigator.language]): string {
  const known = (c: string) => LANGUAGES.some((l) => l.code === c);
  if (pref && pref !== "auto") return known(pref) ? pref : "en";
  for (const tag of system) {
    const base = tag.toLowerCase().split("-")[0];
    if (known(base)) return base;
  }
  return "en";
}

let current = "en";

/** Switch the language used by `t` (and tell the page which one it is). */
export function setLanguage(code: string): void {
  current = code;
  if (typeof document !== "undefined") document.documentElement.lang = code;
}

export const currentLanguage = () => current;

/** Translate an English string. `{name}` placeholders are filled from `vars`. */
export function translate(code: string, en: string, vars?: Record<string, string | number>): string {
  const text = DICTS[code]?.[en] ?? en;
  return vars ? text.replace(/\{(\w+)\}/g, (m, k: string) => (k in vars ? String(vars[k]) : m)) : text;
}

export const t = (en: string, vars?: Record<string, string | number>) => translate(current, en, vars);

/**
 * Use in a component that shows text: it re-renders when the language changes and
 * returns `t` for the language in force. The first caller in a render also makes
 * sure the module-level `t` agrees, so helpers outside React stay in step.
 */
export function useT(): typeof t {
  const pref = useStore((s) => (s.settings ? adv<string>(s.settings, "ui.language") : "auto"));
  const code = resolveLanguage(pref);
  if (code !== current) setLanguage(code);
  return (en, vars) => translate(code, en, vars);
}
