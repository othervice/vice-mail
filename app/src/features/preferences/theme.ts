import type { ThemePreference } from "../../services/ipc";

export type Theme = Exclude<ThemePreference, "system">;

export const DARK_SCHEME_QUERY = "(prefers-color-scheme: dark)";

export function systemPrefersDark(): boolean {
  return window.matchMedia(DARK_SCHEME_QUERY).matches;
}

export function resolveTheme(preference: ThemePreference, systemIsDark: boolean): Theme {
  if (preference !== "system") return preference;
  return systemIsDark ? "dark" : "light";
}

/** tokens.css switches all colours on this attribute. */
export function applyTheme(theme: Theme): void {
  document.documentElement.dataset["theme"] = theme;
}
