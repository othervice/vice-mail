import { hashKey, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { uiIpc, type CoreError, type CoreEvent, type CoreWindowLayout, type UiPreferences } from "../../services/ipc";
import { useLayoutStore } from "../../stores/layout";
import { usePaletteStore } from "../../stores/palette";
import { useShortcutStore } from "../../stores/shortcuts";
import { migrateLocalFallback } from "./migrateLocal";
import { connectUiPreferences, overridesFromCore, uiPreferencesQueryKey, windowLayoutQueryKey } from "./uiPreferences";

/** What the core holds goes into the stores the key handlers and components read. */
function applyUiPreferences(preferences: UiPreferences): void {
  useShortcutStore.getState().applyStored(overridesFromCore(preferences.shortcuts));
  usePaletteStore.getState().applyStored(preferences.recentCommands);
  useLayoutStore.getState().applyStoredTextSize(preferences.textSize);
}

/** `uiPreferencesChanged`: another window or (via the settings sync) another device saved them. */
export function handleUiPreferencesEvent(queryClient: QueryClient, event: CoreEvent): void {
  if (event.type === "uiPreferencesChanged") queryClient.setQueryData(uiPreferencesQueryKey, event.preferences);
}

const PREFERENCES_HASH = hashKey(uiPreferencesQueryKey);
const LAYOUT_HASH = hashKey(windowLayoutQueryKey);

/** One migration per page (StrictMode runs effects twice in development). */
let migration: Promise<unknown> | null = null;

/** For tests: the next render may migrate again. */
export function resetMigrationForTests(): void {
  migration = null;
}

export interface UiPreferencesStatus {
  /** Both answers arrived (data or an error); the frame waits for it to avoid a jump in layout. */
  settled: boolean;
  preferencesError: CoreError | null;
  layoutError: CoreError | null;
}

/** The UI preferences as the core holds them (loaded once; events and saves keep the cache current). */
export function useUiPreferencesLoad() {
  return useQuery({ queryKey: uiPreferencesQueryKey, queryFn: uiIpc.getUiPreferences, staleTime: Infinity });
}

/** The layout of this window as the core holds it. */
export function useWindowLayoutLoad() {
  return useQuery({ queryKey: windowLayoutQueryKey, queryFn: uiIpc.getWindowLayout, staleTime: Infinity });
}

const settledOf = (query: { dataUpdatedAt: number; errorUpdatedAt: number }) => query.dataUpdatedAt > 0 || query.errorUpdatedAt > 0;

/**
 * Stage 8: loads the UI preferences and this window's layout from the core, keeps the stores in
 * step with the cache (optimistic saves, rollbacks, `uiPreferencesChanged`) and, in the main
 * window, moves the values of the old local fallback to the core once.
 */
export function useUiPreferencesSync(options: { migrate: boolean }): UiPreferencesStatus {
  const queryClient = useQueryClient();

  // The cache tells synchronously when data lands, before the frame renders with it.
  useEffect(() => {
    connectUiPreferences(queryClient);
    const preferences = queryClient.getQueryData<UiPreferences>(uiPreferencesQueryKey);
    if (preferences) applyUiPreferences(preferences);
    const layout = queryClient.getQueryData<CoreWindowLayout>(windowLayoutQueryKey);
    if (layout) useLayoutStore.getState().applyStoredLayout(layout);
    const unsubscribe = queryClient.getQueryCache().subscribe((event) => {
      if (event.type !== "updated" || event.action.type !== "success") return;
      const hash = event.query.queryHash;
      const data: unknown = event.query.state.data;
      if (data === undefined) return;
      if (hash === PREFERENCES_HASH) applyUiPreferences(data as UiPreferences);
      else if (hash === LAYOUT_HASH) useLayoutStore.getState().applyStoredLayout(data as CoreWindowLayout);
    });
    return () => {
      unsubscribe();
      connectUiPreferences(null);
    };
  }, [queryClient]);

  const preferences = useUiPreferencesLoad();
  const layout = useWindowLayoutLoad();

  const loadedPreferences = preferences.data;
  const loadedLayout = layout.data;
  useEffect(() => {
    if (!options.migrate || !loadedPreferences || !loadedLayout || migration) return;
    migration = migrateLocalFallback(loadedPreferences, loadedLayout).catch((error: unknown) => {
      // The local copy stays for the next start; the settings show why it did not move.
      useShortcutStore.setState({ saveError: error as CoreError });
    });
  }, [options.migrate, loadedPreferences, loadedLayout]);

  return {
    settled: settledOf(preferences) && settledOf(layout),
    preferencesError: preferences.error,
    layoutError: layout.error,
  };
}
