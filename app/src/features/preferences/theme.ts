import { useSyncExternalStore } from "react";
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

const themeListeners = new Set<() => void>();

/** tokens.css switches all colours on this attribute. */
export function applyTheme(theme: Theme): void {
  document.documentElement.dataset["theme"] = theme;
  for (const listener of themeListeners) listener();
}

function appliedTheme(): Theme {
  return document.documentElement.dataset["theme"] === "dark" ? "dark" : "light";
}

function subscribeTheme(listener: () => void): () => void {
  themeListeners.add(listener);
  return () => {
    themeListeners.delete(listener);
  };
}

/**
 * The theme the window shows now (what `applyTheme` set last). Stage 21 (decision 0066): the
 * reading pane asks the core for a mail in this scheme.
 */
export function useAppliedTheme(): Theme {
  return useSyncExternalStore(subscribeTheme, appliedTheme, () => "light");
}
