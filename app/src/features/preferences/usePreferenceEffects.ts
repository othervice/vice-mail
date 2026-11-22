import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { resolveLanguage } from "../../i18n";
import type { Preferences } from "../../services/ipc";
import { applyTheme, DARK_SCHEME_QUERY, resolveTheme } from "./theme";

/**
 * Applies theme and language to the document as soon as the preferences change, without a reload.
 * With "system" it keeps following the operating system while the app runs.
 */
export function usePreferenceEffects(preferences: Preferences): void {
  const { theme, language } = preferences;
  const { i18n } = useTranslation();

  useEffect(() => {
    const media = window.matchMedia(DARK_SCHEME_QUERY);
    const apply = () => {
      applyTheme(resolveTheme(theme, media.matches));
    };
    apply();
    if (theme !== "system") return undefined;
    media.addEventListener("change", apply);
    return () => {
      media.removeEventListener("change", apply);
    };
  }, [theme]);

  useEffect(() => {
    const apply = () => {
      const resolved = resolveLanguage(language, navigator.languages);
      document.documentElement.lang = resolved;
      if (i18n.language !== resolved) {
        // Resources are bundled, so this switches synchronously and cannot fail to load.
        void i18n.changeLanguage(resolved);
      }
    };
    apply();
    if (language !== "system") return undefined;
    window.addEventListener("languagechange", apply);
    return () => {
      window.removeEventListener("languagechange", apply);
    };
  }, [language, i18n]);
}
