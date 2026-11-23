import type { CoreWindowLayout, UiPreferences } from "../../services/ipc";
import { DEFAULT_LAYOUT, parseStoredLayout } from "../../stores/layout";
import { parseStoredRecent } from "../../stores/palette";
import { parseStoredOverrides } from "../../stores/shortcuts";
import { readLocal, removeLocal, type LocalKey } from "./localFallback";
import { DEFAULT_UI_PREFERENCES, saveUiPreferences, saveWindowLayout, type UiPreferencesChange } from "./uiPreferences";

/** The keys the stage 8 fallback used (the main window's layout under `layout.main`). */
const LEGACY_KEYS: readonly LocalKey[] = ["shortcuts", "paletteRecent", "layout.main"];

function isDefaultPreferences(preferences: UiPreferences): boolean {
  return (
    preferences.shortcuts.macos.length === 0 &&
    preferences.shortcuts.windows.length === 0 &&
    preferences.textSize === DEFAULT_UI_PREFERENCES.textSize &&
    preferences.recentCommands.length === 0
  );
}

function isDefaultLayout(layout: CoreWindowLayout): boolean {
  return (
    layout.readingPane === DEFAULT_LAYOUT.readingPane &&
    layout.density === DEFAULT_LAYOUT.density &&
    layout.sidebarExpanded === DEFAULT_LAYOUT.sidebarExpanded
  );
}

/**
 * Stage 8, once per device: an earlier version kept shortcut changes, the recent palette
 * commands and the main window's layout in the web view. If the core has nothing yet, those
 * values go to the core; then the local copy is removed. If the core already has values (this
 * device or another one via the sync), the core wins and the local copy is removed too. When
 * writing fails, the local copy stays for the next start and the error comes back.
 *
 * Only the main window migrates (the fallback only ever stored its layout).
 */
export async function migrateLocalFallback(preferences: UiPreferences, layout: CoreWindowLayout): Promise<boolean> {
  const raw = LEGACY_KEYS.map((key) => readLocal(key));
  if (raw.every((value) => value === null)) return false;
  const [shortcuts, recent, legacyLayout] = raw;

  const change: UiPreferencesChange = {};
  if (isDefaultPreferences(preferences)) {
    const overrides = parseStoredOverrides(shortcuts);
    if (Object.keys(overrides.macos).length > 0 || Object.keys(overrides.windows).length > 0) change.overrides = overrides;
    const commands = parseStoredRecent(recent);
    if (commands.length > 0) change.recentCommands = commands;
    if (legacyLayout !== null) {
      const { textSize } = parseStoredLayout(legacyLayout);
      if (textSize !== DEFAULT_UI_PREFERENCES.textSize) change.textSize = textSize;
    }
  }
  const stored = legacyLayout === null ? null : parseStoredLayout(legacyLayout);
  const layoutChange =
    stored && isDefaultLayout(layout) && !isDefaultLayout(stored)
      ? { readingPane: stored.readingPane, density: stored.density, sidebarExpanded: stored.sidebarExpanded }
      : null;

  if (Object.keys(change).length > 0) await saveUiPreferences(change);
  if (layoutChange) await saveWindowLayout(layoutChange);
  for (const key of LEGACY_KEYS) removeLocal(key);
  return true;
}
