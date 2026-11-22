import i18next, { type i18n as I18n } from "i18next";
import type { LanguagePreference } from "../services/ipc";
import { de } from "./de";
import { en } from "./en";

export type Language = Exclude<LanguagePreference, "system">;

export const LANGUAGES = ["en", "de"] as const satisfies readonly Language[];

export const resources = {
  en: { translation: en },
  de: { translation: de },
} as const;

declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "translation";
    resources: { translation: typeof en };
  }
}

/**
 * Creates a ready-to-use i18next instance. Resources are bundled, so initialisation is
 * synchronous and `t` works right after this returns. One instance per app (and per test).
 */
export function createI18n(language: Language): I18n {
  const instance = i18next.createInstance();
  void instance.init({
    resources,
    lng: language,
    fallbackLng: "en",
    supportedLngs: LANGUAGES,
    // React escapes output itself.
    interpolation: { escapeValue: false },
    initAsync: false,
  });
  return instance;
}

/**
 * The language the UI shows for a preference. "system" takes the first of the system's preferred
 * languages that the app supports and falls back to English.
 */
export function resolveLanguage(
  preference: LanguagePreference,
  systemLanguages: readonly string[],
): Language {
  if (preference !== "system") return preference;
  for (const tag of systemLanguages) {
    const primary = tag.toLowerCase().split("-")[0];
    const match = LANGUAGES.find((language) => language === primary);
    if (match) return match;
  }
  return "en";
}
