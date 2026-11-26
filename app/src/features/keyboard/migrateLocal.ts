import type { CoreWindowLayout, UiPreferences } from "../../services/ipc";
import { DEFAULT_LAYOUT, parseStoredLayout } from "../../stores/layout";
import { parseStoredRecent } from "../../stores/palette";
import { parseStoredOverrides } from "../../stores/shortcuts";
import { isAccentTheme } from "../preferences/accent";
import { readLocal, removeLocal, type LocalKey } from "./localFallback";
import { DEFAULT_UI_PREFERENCES, saveUiPreferences, saveWindowLayout, type UiPreferencesChange } from "./uiPreferences";

/**
 * The keys the stage 8 fallback used (the main window's layout under `layout.main`), and the AI
 * panel's state of stage 13 until decision 0028.
 */
const LEGACY_KEYS: readonly LocalKey[] = ["shortcuts", "paletteRecent", "layout.main", "layout.aiPanel"];

function isDefaultPreferences(preferences: UiPreferences): boolean {
  return (
    preferences.shortcuts.macos.length === 0 &&
    preferences.shortcuts.windows.length === 0 &&
    preferences.textSize === DEFAULT_UI_PREFERENCES.textSize &&
    preferences.recentCommands.length === 0 &&
    preferences.aiPanel === DEFAULT_UI_PREFERENCES.aiPanel
  );
}

/**
 * Whether a layout is what nothing stored looks like. The sidebar does not count: the core's
 * default for it changed with the accounts-and-folders column (shown) after older cores had
 * stored the folded rail as their default.
 */
function isDefaultLayout(layout: CoreWindowLayout): boolean {
  return layout.readingPane === DEFAULT_LAYOUT.readingPane && layout.density === DEFAULT_LAYOUT.density;
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
  const [shortcuts, recent, legacyLayout, aiPanel] = raw;

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
    // The panel was stored as `open: boolean`; only "folded" differs from the default.
    if (aiPanel === false) change.aiPanel = "folded";
  }
  const stored = legacyLayout === null ? null : parseStoredLayout(legacyLayout);
  const layoutChange =
    stored && isDefaultLayout(layout) && !isDefaultLayout(stored)
      ? {
          readingPane: stored.readingPane,
          density: stored.density,
          sidebarExpanded: stored.sidebarExpanded,
          // The local fallback had no column width nor list size; the core's layout keeps its own.
          sidebarWidth: layout.sidebarWidth,
          listWidth: layout.listWidth,
          listHeight: layout.listHeight,
          contactPanel: layout.contactPanel,
        }
      : null;

  if (Object.keys(change).length > 0) await saveUiPreferences(change);
  if (layoutChange) await saveWindowLayout(layoutChange);
  for (const key of LEGACY_KEYS) removeLocal(key);
  return true;
}

/**
 * Stage 16, once per device (decision 0036): until the core stored the accent theme, the
 * choice lived in the web view of this device. A valid theme there goes to the core (when it
 * differs), then the local copy is removed; an invalid one is just removed. When writing fails,
 * the local copy stays for the next start and the error comes back.
 */
export async function migrateLocalAccent(preferences: UiPreferences): Promise<boolean> {
  const stored = readLocal("accentTheme");
  if (stored === null) return false;
  if (isAccentTheme(stored) && stored !== preferences.accentTheme) await saveUiPreferences({ accentTheme: stored });
  removeLocal("accentTheme");
  return true;
}
