import i18next from "i18next";
import { initReactI18next } from "react-i18next";

import de from "./locales/de.json";
import en from "./locales/en.json";

export const LANGUAGES = ["en", "de"] as const;
export type Language = (typeof LANGUAGES)[number];

/** What the setting can hold: a concrete language, or "follow whatever the system says". */
export type LanguageSetting = Language | "system";

export const DEFAULT_LANGUAGE: Language = "en";

/** Maps something like "de", "de-DE" or "de-AT" onto the dictionaries that exist. */
export function resolveLanguage(setting: LanguageSetting | null | undefined): Language {
  if (setting && setting !== "system" && LANGUAGES.includes(setting)) return setting;

  const tags = typeof navigator === "undefined" ? [] : [navigator.language, ...(navigator.languages ?? [])];
  for (const tag of tags) {
    if (!tag) continue;
    const base = tag.toLowerCase().split("-")[0];
    if (base === "de") return "de";
    if (base === "en") return "en";
  }
  return DEFAULT_LANGUAGE;
}

void i18next.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    de: { translation: de },
  },
  lng: DEFAULT_LANGUAGE,
  fallbackLng: DEFAULT_LANGUAGE,
  interpolation: {
    // React already escapes everything it renders; doing it again turns quotes and dashes into entities.
    escapeValue: false,
  },
});

export function applyLanguage(setting: LanguageSetting | null | undefined): Language {
  const language = resolveLanguage(setting);
  if (i18next.language !== language) void i18next.changeLanguage(language);
  document.documentElement.lang = language.replace("_", "-");
  return language;
}

export { i18next };
