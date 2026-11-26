import { useMemo } from "react";
import { create } from "zustand";
import { MAX_COLUMN_VIEWS, toCoreError, type ColumnView, type CoreError } from "../../services/ipc";
import type { ShownMailViewId } from "../../stores/ui";
import { saveUiPreferences } from "../keyboard/uiPreferences";

/**
 * Stage 17: the views of the column (docs/DESIGN.md, "Seitenspalte"). The one list of them: the
 * column, the rail, the editor ("Edit views…") and the core's preference
 * (`UiPreferences.columnViews`) all go by it. Inbox and Sent of every account and of all accounts
 * are fixed rows of the column and are not part of it.
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

/**
 * Stage 23 (decision 0085): the line between the views of "All accounts" and those under "More",
 * an entry of the same list (the core stores it like a view, older versions keep it as one they do
 * not know). The views before it show in "All accounts", the ones after it under "More".
 */
export const MORE_ENTRY = "more";

export type ColumnEntryId = ColumnViewId | typeof MORE_ENTRY;

export function isColumnViewId(id: string): id is ColumnViewId {
  return (COLUMN_VIEW_IDS as readonly string[]).includes(id);
}

function isColumnEntryId(id: string): id is ColumnEntryId {
  return id === MORE_ENTRY || isColumnViewId(id);
}

/**
 * Stage 23 (decision 0085), the approved draft: Later and Flagged in "All accounts" (between the
 * inbox and Sent), every other view under "More", in the order of stage 17.
 */
const DEFAULT_MAIN: readonly ColumnViewId[] = ["later", "flagged"];

/** Every entry in its default place: the main views, the line of "More", the other views. */
export const DEFAULT_COLUMN_ORDER: readonly ColumnEntryId[] = [
  ...DEFAULT_MAIN,
  MORE_ENTRY,
  ...COLUMN_VIEW_IDS.filter((id) => !DEFAULT_MAIN.includes(id)),
];

/** One entry of the editor and the column: a view and whether it shows, or the line of "More". */
export interface ColumnViewChoice {
  id: ColumnEntryId;
  shown: boolean;
}

/** Every view shown, in the default order. */
export const DEFAULT_COLUMN_VIEWS: readonly ColumnViewChoice[] = DEFAULT_COLUMN_ORDER.map((id) => ({ id, shown: true }));

/**
 * The core's list as the full choice: its known entries in its order, then every entry it does
 * not name at its default place (after the entry it follows by default) and shown. Entries of
 * views this version does not know (a newer device) are left out here and kept by
 * `columnViewsToCore`. A list saved before stage 23 has no line of "More": Later and Flagged then
 * go to "All accounts" (in the user's order), every other view under "More", each keeping its
 * place and visibility.
 */
export function resolveColumnViews(stored: readonly ColumnView[]): ColumnViewChoice[] {
  let result: ColumnViewChoice[] = [];
  for (const entry of stored) {
    if (isColumnEntryId(entry.id) && !result.some((choice) => choice.id === entry.id)) {
      result.push({ id: entry.id, shown: entry.id === MORE_ENTRY || entry.shown });
    }
  }
  if (result.length > 0 && !result.some((choice) => choice.id === MORE_ENTRY)) {
    const main = (choice: ColumnViewChoice) => (DEFAULT_MAIN as readonly string[]).includes(choice.id);
    result = [...result.filter(main), { id: MORE_ENTRY, shown: true }, ...result.filter((choice) => !main(choice))];
  }
  DEFAULT_COLUMN_ORDER.forEach((id, index) => {
    if (result.some((choice) => choice.id === id)) return;
    // After the nearest entry that comes before it by default, else at the top.
    const before = DEFAULT_COLUMN_ORDER.slice(0, index)
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
 * its place), else every entry in order, followed by the entries of views this version does not
 * know (`previous`), within the core's limit.
 */
export function columnViewsToCore(choice: readonly ColumnViewChoice[], previous: readonly ColumnView[]): ColumnView[] {
  const foreign = previous.filter((entry) => !isColumnEntryId(entry.id));
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

/** The views shown in "All accounts" and under "More", each in the user's order. */
export interface ColumnGroups {
  main: ColumnViewId[];
  more: ColumnViewId[];
}

/** The shown views on either side of the line of "More" (stage 23, decision 0085). */
export function columnGroups(views: readonly ColumnViewChoice[]): ColumnGroups {
  const line = views.findIndex((view) => view.id === MORE_ENTRY);
  const shown = (part: readonly ColumnViewChoice[]) =>
    part.flatMap((view) => (view.shown && view.id !== MORE_ENTRY ? [view.id] : []));
  return line < 0 ? { main: shown(views), more: [] } : { main: shown(views.slice(0, line)), more: shown(views.slice(line + 1)) };
}

/** The views the column and the rail show, in "All accounts" and under "More". */
export function useColumnGroups(): ColumnGroups {
  const views = useColumnViews();
  return useMemo(() => columnGroups(views), [views]);
}
