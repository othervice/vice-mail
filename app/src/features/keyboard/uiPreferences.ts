import type { QueryClient } from "@tanstack/react-query";
import {
  DEFAULT_TEXT_SIZE,
  MAX_RECENT_COMMAND_CHARS,
  MAX_RECENT_COMMANDS,
  MAX_TEXT_SIZE,
  MIN_TEXT_SIZE,
  toCoreError,
  uiIpc,
  type CoreWindowLayout,
  type PlatformShortcuts,
  type ShortcutOverride,
  type UiPreferences,
} from "../../services/ipc";
import type { Shortcut, ShortcutPlatform } from "../actions/shortcuts";
import { isCommandId, parseOverrides, type ShortcutOverrides } from "./bindings";

/**
 * Stage 8 (decision 0025): the UI preferences live in the core. Shortcut changes and the text
 * size are synced between devices, the recent palette commands stay on the device; the layout
 * belongs to the window. The stores (`stores/shortcuts.ts`, `palette.ts`, `layout.ts`) hold what
 * key handlers read at once; this module saves their changes through the query cache
 * (optimistic, rolled back when the core refuses) and `useUiPreferencesSync` applies what the
 * core answers or announces (`uiPreferencesChanged`: another window or another device).
 */
export const uiPreferencesQueryKey = ["uiPreferences"] as const;
export const windowLayoutQueryKey = ["windowLayout"] as const;

export const DEFAULT_UI_PREFERENCES: UiPreferences = {
  shortcuts: { macos: [], windows: [] },
  textSize: DEFAULT_TEXT_SIZE,
  recentCommands: [],
};

export type PlatformOverrides = Record<ShortcutPlatform, ShortcutOverrides>;

const PLATFORMS: readonly ShortcutPlatform[] = ["macos", "windows"];

/** The core's lists (`{ command, shortcuts }`) as the UI's map; unknown commands are left out. */
export function overridesFromCore(shortcuts: PlatformShortcuts): PlatformOverrides {
  const map = (list: readonly ShortcutOverride[]) =>
    parseOverrides(Object.fromEntries(list.map((entry) => [entry.command, entry.shortcuts])));
  return { macos: map(shortcuts.macos), windows: map(shortcuts.windows) };
}

const toCoreShortcut = (shortcut: Shortcut) => ({ key: shortcut.key, shift: shortcut.shift === true, mod: shortcut.mod === true });

/**
 * The UI's map as the core's lists, sorted by command like the core stores them. Entries of
 * commands this version does not know (written by a newer version on another device) are kept.
 */
export function overridesToCore(overrides: PlatformOverrides, previous: PlatformShortcuts): PlatformShortcuts {
  const convert = (platform: ShortcutPlatform): ShortcutOverride[] => {
    const own = Object.entries(overrides[platform]).flatMap(([command, list]) =>
      list ? [{ command, shortcuts: list.map(toCoreShortcut) }] : [],
    );
    const foreign = previous[platform].filter((entry) => !isCommandId(entry.command));
    return [...own, ...foreign].sort((a, b) => a.command.localeCompare(b.command));
  };
  return { macos: convert("macos"), windows: convert("windows") };
}

/** Any whole percent from 80 to 150 is valid in the core; the UI offers a few of them. */
export function clampTextSize(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_TEXT_SIZE;
  return Math.min(MAX_TEXT_SIZE, Math.max(MIN_TEXT_SIZE, Math.round(value)));
}

/** The recent list the core accepts: newest first, no duplicates, at most five, each 1..200. */
export function cleanRecent(recent: readonly string[]): string[] {
  const seen = new Set<string>();
  const kept: string[] = [];
  for (const entry of recent) {
    if (entry.length === 0 || entry.length > MAX_RECENT_COMMAND_CHARS || seen.has(entry)) continue;
    // Control characters are refused by the core (they never occur in the palette's keys).
    if (Array.from(entry).some((c) => c.charCodeAt(0) < 0x20 || c.charCodeAt(0) === 0x7f)) continue;
    seen.add(entry);
    kept.push(entry);
  }
  return kept.slice(0, MAX_RECENT_COMMANDS);
}

/** What `useUiPreferencesSync` connects: the query cache of this window. */
let client: QueryClient | null = null;

export function connectUiPreferences(queryClient: QueryClient | null): void {
  client = queryClient;
}

/** The change the stores make: the shortcuts as the UI's map, the rest as the core's fields. */
export interface UiPreferencesChange {
  overrides?: PlatformOverrides;
  textSize?: number;
  recentCommands?: readonly string[];
}

let saves: Promise<unknown> = Promise.resolve();

/**
 * Saves one change: the cache shows it at once, the core gets the whole object (the parts not
 * changed as they are). A refusal puts the previous value back and rejects with the typed error.
 * Saves run one after another, so a later change builds on the earlier one.
 */
export function saveUiPreferences(change: UiPreferencesChange): Promise<UiPreferences> {
  const run = saves.then(async () => {
    const queryClient = client;
    let previous: UiPreferences;
    try {
      // Not loaded yet (or the load failed): ask first, never overwrite unknown values with defaults.
      previous =
        queryClient?.getQueryData<UiPreferences>(uiPreferencesQueryKey) ??
        (queryClient
          ? await queryClient.fetchQuery({ queryKey: uiPreferencesQueryKey, queryFn: uiIpc.getUiPreferences })
          : await uiIpc.getUiPreferences());
    } catch (e) {
      throw toCoreError(e);
    }
    const next: UiPreferences = {
      shortcuts: change.overrides ? overridesToCore(change.overrides, previous.shortcuts) : previous.shortcuts,
      textSize: change.textSize === undefined ? previous.textSize : clampTextSize(change.textSize),
      recentCommands: change.recentCommands ? cleanRecent(change.recentCommands) : previous.recentCommands,
    };
    await queryClient?.cancelQueries({ queryKey: uiPreferencesQueryKey });
    queryClient?.setQueryData(uiPreferencesQueryKey, next);
    try {
      const saved = await uiIpc.setUiPreferences(next);
      queryClient?.setQueryData(uiPreferencesQueryKey, saved);
      return saved;
    } catch (e) {
      queryClient?.setQueryData(uiPreferencesQueryKey, previous);
      throw toCoreError(e);
    }
  });
  saves = run.catch(() => undefined);
  return run;
}

let layoutSaves: Promise<unknown> = Promise.resolve();

/** The layout of this window, saved the same way (optimistic, back to the previous on a refusal). */
export function saveWindowLayout(layout: CoreWindowLayout): Promise<CoreWindowLayout> {
  const run = layoutSaves.then(async () => {
    const queryClient = client;
    const previous = queryClient?.getQueryData<CoreWindowLayout>(windowLayoutQueryKey);
    await queryClient?.cancelQueries({ queryKey: windowLayoutQueryKey });
    queryClient?.setQueryData(windowLayoutQueryKey, layout);
    try {
      const saved = await uiIpc.setWindowLayout(layout);
      queryClient?.setQueryData(windowLayoutQueryKey, saved);
      return saved;
    } catch (e) {
      if (previous) queryClient?.setQueryData(windowLayoutQueryKey, previous);
      throw toCoreError(e);
    }
  });
  layoutSaves = run.catch(() => undefined);
  return run;
}

/** For tests: forget the connected cache and queued saves between renders. */
export function resetUiPreferencesForTests(): void {
  client = null;
  saves = Promise.resolve();
  layoutSaves = Promise.resolve();
}

export { PLATFORMS };
