import { useMemo } from "react";
import { create } from "zustand";
import { MAX_COLUMN_VIEWS, toCoreError, type ColumnView, type CoreError } from "../../services/ipc";
import type { ShownMailViewId } from "../../stores/ui";
import { saveUiPreferences } from "../keyboard/uiPreferences";

/**
 * Stage 17: the views of the column's "Views" section, in their default order (docs/DESIGN.md,
 * "Seitenspalte"). The one list of them: the column, the rail, the editor ("Edit views…") and the
 * core's preference (`UiPreferences.columnViews`) all go by it. Inbox, Sent, Drafts, Archive and
 * Trash are in every account's tree and are not part of it.
 */
export const COLUMN_VIEW_IDS = [
  "later",
  // Stage 20 (decision 0059): "Waiting for reply", after Later.
  "waiting",
  "tasks",
  "readingList",
  "pinnedAttachments",
  // Stage 20 (decision 0060): every attachment, after the pinned ones.
  "attachments",
  "flagged",
  "pinned",
  "savedViews",
  // Stage 17 (decision 0050): the contacts, after the saved searches.
  "contacts",
] as const satisfies readonly ShownMailViewId[];

export type ColumnViewId = (typeof COLUMN_VIEW_IDS)[number];

export function isColumnViewId(id: string): id is ColumnViewId {
  return (COLUMN_VIEW_IDS as readonly string[]).includes(id);
}

/** One view of the editor and the column: its id and whether it shows. */
export interface ColumnViewChoice {
  id: ColumnViewId;
  shown: boolean;
}

/** Every view shown, in the default order. */
export const DEFAULT_COLUMN_VIEWS: readonly ColumnViewChoice[] = COLUMN_VIEW_IDS.map((id) => ({ id, shown: true }));

/**
 * The core's list as the full choice: its known views in its order, then every view it does not
 * name at its default place (after the view it follows by default) and shown. Entries of views
 * this version does not know (a newer device) are left out here and kept by `columnViewsToCore`.
 */
export function resolveColumnViews(stored: readonly ColumnView[]): ColumnViewChoice[] {
  const result: ColumnViewChoice[] = [];
  for (const entry of stored) {
    if (isColumnViewId(entry.id) && !result.some((choice) => choice.id === entry.id)) {
      result.push({ id: entry.id, shown: entry.shown });
    }
  }
  COLUMN_VIEW_IDS.forEach((id, index) => {
    if (result.some((choice) => choice.id === id)) return;
    // After the nearest view that comes before it by default, else at the top.
    const before = COLUMN_VIEW_IDS.slice(0, index)
      .reverse()
      .find((earlier) => result.some((choice) => choice.id === earlier));
    const at = before === undefined ? 0 : result.findIndex((choice) => choice.id === before) + 1;
    result.splice(at, 0, { id, shown: true });
  });
  return result;
}

/** Whether a choice is the default (every view shown, in the default order). */
export function isDefaultColumnViews(choice: readonly ColumnViewChoice[]): boolean {
  return (
    choice.length === DEFAULT_COLUMN_VIEWS.length &&
    choice.every((entry, index) => entry.shown && entry.id === DEFAULT_COLUMN_VIEWS[index]?.id)
  );
}

/**
 * A choice as the core stores it: empty for the default (so a view a later version adds shows at
 * its place), else every view in order, followed by the entries of views this version does not
 * know (`previous`), within the core's limit.
 */
export function columnViewsToCore(choice: readonly ColumnViewChoice[], previous: readonly ColumnView[]): ColumnView[] {
  const foreign = previous.filter((entry) => !isColumnViewId(entry.id));
  if (foreign.length === 0 && isDefaultColumnViews(choice)) return [];
  const own = choice.map(({ id, shown }) => ({ id, shown }));
  const seen = new Set<string>(own.map((entry) => entry.id));
  const kept = foreign.filter((entry) => {
    if (seen.has(entry.id)) return false;
    seen.add(entry.id);
    return true;
  });
  return [...own, ...kept].slice(0, MAX_COLUMN_VIEWS);
}

/** The list with the entry at `from` moved to `to` (both inside the list). */
export function moveColumnView<T>(list: readonly T[], from: number, to: number): T[] {
  const next = [...list];
  if (from < 0 || from >= next.length || to < 0 || to >= next.length) return next;
  const [moved] = next.splice(from, 1);
  if (moved !== undefined) next.splice(to, 0, moved);
  return next;
}

interface ColumnViewsState {
  /** What the core holds (`UiPreferences.columnViews`); empty is the default. */
  stored: readonly ColumnView[];
  /** While a view is dragged in the editor: the order shown meanwhile (not saved yet). */
  preview: readonly ColumnViewChoice[] | null;
  /** The core refused the last change; the previous choice applies again. */
  saveError: CoreError | null;
  /** "Edit views…" is open. */
  editing: boolean;
  /** The user's choice: shown at once in the column and the rail, saved to the core. */
  choose: (choice: readonly ColumnViewChoice[]) => void;
  /** "Reset to default": every view, in the default order. */
  reset: () => void;
  setPreview: (preview: readonly ColumnViewChoice[] | null) => void;
  /** What the core holds (loaded, saved, or announced by `uiPreferencesChanged`). */
  applyStored: (views: unknown) => void;
  openEditor: () => void;
  closeEditor: () => void;
}

const isColumnViewList = (value: unknown): value is ColumnView[] =>
  Array.isArray(value) &&
  value.every((entry: unknown) => {
    if (typeof entry !== "object" || entry === null) return false;
    const { id, shown } = entry as { id?: unknown; shown?: unknown };
    return typeof id === "string" && typeof shown === "boolean";
  });

/**
 * Stage 17: which views the column and the rail show, in which order. A UI preference of the
 * core, synced between devices like the text size; `useUiPreferencesSync` fills the store from
 * the core (load, saves, `uiPreferencesChanged`).
 */
export const useColumnViewsStore = create<ColumnViewsState>()((set, get) => {
  const save = (next: ColumnView[]) => {
    set({ stored: next, preview: null, saveError: null });
    saveUiPreferences({ columnViews: next }).catch((error: unknown) => {
      // The cache is back at the previous value; `applyStored` has put it on screen again.
      set({ saveError: toCoreError(error) });
    });
  };
  return {
    stored: [],
    preview: null,
    saveError: null,
    editing: false,
    choose: (choice) => {
      save(columnViewsToCore(choice, get().stored));
    },
    reset: () => {
      save(columnViewsToCore(DEFAULT_COLUMN_VIEWS, get().stored));
    },
    setPreview: (preview) => {
      set({ preview });
    },
    applyStored: (views) => {
      set({ stored: isColumnViewList(views) ? views : [] });
    },
    openEditor: () => {
      set({ editing: true, saveError: null });
    },
    closeEditor: () => {
      set({ editing: false, preview: null });
    },
  };
});

/** Every view of the column with whether it shows, in the user's order (the drag's preview first). */
export function useColumnViews(): readonly ColumnViewChoice[] {
  const stored = useColumnViewsStore((state) => state.stored);
  const preview = useColumnViewsStore((state) => state.preview);
  const resolved = useMemo(() => resolveColumnViews(stored), [stored]);
  return preview ?? resolved;
}

/** The views the column and the rail show, in the user's order. */
export function useShownColumnViews(): ColumnViewId[] {
  const views = useColumnViews();
  return useMemo(() => views.filter((view) => view.shown).map((view) => view.id), [views]);
}
