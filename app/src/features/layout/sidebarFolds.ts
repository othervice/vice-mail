import { useCallback } from "react";
import { create } from "zustand";
import { MAX_SIDEBAR_FOLDS, toCoreError, type AccountId, type CoreError, type SidebarFold } from "../../services/ipc";
import { saveUiPreferences } from "../keyboard/uiPreferences";

/**
 * Stage 23 (decision 0085): the parts of the sidebar that open and close, by their key in the
 * core's list (`UiPreferences.sidebarFolds`, on this device): the sections "All accounts" and
 * "Favourites", "More", each account's section and its "More folders".
 */
export type FoldKey = "allAccounts" | "more" | "favorites" | `account:${AccountId}` | `folders:${AccountId}`;

export const accountFold = (id: AccountId): FoldKey => `account:${id}`;
export const foldersFold = (id: AccountId): FoldKey => `folders:${id}`;

/** How each part shows until the user opens or closes it: "More" and "More folders" closed. */
export function defaultOpen(key: FoldKey): boolean {
  return key !== "more" && !key.startsWith("folders:");
}

const isFoldList = (value: unknown): value is SidebarFold[] =>
  Array.isArray(value) &&
  value.every((entry: unknown) => {
    if (typeof entry !== "object" || entry === null) return false;
    const { key, open } = entry as { key?: unknown; open?: unknown };
    return typeof key === "string" && typeof open === "boolean";
  });

/** The account a key belongs to, if it is an account's part. */
function accountOf(key: string): string | null {
  const colon = key.indexOf(":");
  return colon < 0 ? null : key.slice(colon + 1);
}

/**
 * The list with `key` set to `open`: only parts that differ from their default are kept (the
 * newest last), parts of accounts that are gone are dropped, and the oldest go first should the
 * list outgrow the core's limit.
 */
export function withFold(list: readonly SidebarFold[], key: FoldKey, open: boolean, accounts: ReadonlySet<string> | null): SidebarFold[] {
  const kept = list.filter((fold) => {
    if (fold.key === key) return false;
    const account = accountOf(fold.key);
    return account === null || accounts === null || accounts.has(account);
  });
  if (open !== defaultOpen(key)) kept.push({ key, open });
  return kept.slice(Math.max(0, kept.length - MAX_SIDEBAR_FOLDS));
}

interface SidebarFoldState {
  /** What the core holds (`UiPreferences.sidebarFolds`); empty is the default. */
  stored: readonly SidebarFold[];
  /** The accounts there are, to drop parts of removed accounts when saving; null until known. */
  accounts: ReadonlySet<string> | null;
  /** The core refused the last change; the previous state applies again. */
  saveError: CoreError | null;
  setFold: (key: FoldKey, open: boolean) => void;
  setAccounts: (ids: readonly AccountId[]) => void;
  /** What the core holds (loaded, saved, or announced by `uiPreferencesChanged`). */
  applyStored: (folds: unknown) => void;
}

/**
 * Which parts of the sidebar are open. Shown at once, saved to the core; `useUiPreferencesSync`
 * fills the store from the core (load, saves, `uiPreferencesChanged`).
 */
export const useSidebarFoldStore = create<SidebarFoldState>()((set, get) => ({
  stored: [],
  accounts: null,
  saveError: null,
  setFold: (key, open) => {
    const next = withFold(get().stored, key, open, get().accounts);
    set({ stored: next, saveError: null });
    saveUiPreferences({ sidebarFolds: next }).catch((error: unknown) => {
      // The cache is back at the previous value; `applyStored` has put it on screen again.
      set({ saveError: toCoreError(error) });
    });
  },
  setAccounts: (ids) => {
    set({ accounts: new Set(ids) });
  },
  applyStored: (folds) => {
    set({ stored: isFoldList(folds) ? folds : [] });
  },
}));

/** Whether a part is open: what the user chose, else its default. */
export function isFoldOpen(stored: readonly SidebarFold[], key: FoldKey): boolean {
  return stored.find((fold) => fold.key === key)?.open ?? defaultOpen(key);
}

/** One part of the sidebar: open or closed, and the way to change it (remembered on this device). */
export function useFold(key: FoldKey): { open: boolean; setOpen: (open: boolean) => void; toggle: () => void } {
  const open = useSidebarFoldStore((state) => isFoldOpen(state.stored, key));
  const setFold = useSidebarFoldStore((state) => state.setFold);
  const setOpen = useCallback(
    (next: boolean) => {
      if (next !== isFoldOpen(useSidebarFoldStore.getState().stored, key)) setFold(key, next);
    },
    [key, setFold],
  );
  const toggle = useCallback(() => {
    setFold(key, !isFoldOpen(useSidebarFoldStore.getState().stored, key));
  }, [key, setFold]);
  return { open, setOpen, toggle };
}
