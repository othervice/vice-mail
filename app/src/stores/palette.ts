import { create } from "zustand";
import { cleanRecent, saveUiPreferences } from "../features/keyboard/uiPreferences";
import type { CoreError } from "../services/ipc";

/** Recent palette commands as the local fallback of an earlier version stored them (migration only). */
export function parseStoredRecent(raw: unknown): string[] {
  if (!Array.isArray(raw)) return [];
  return cleanRecent(raw.filter((entry): entry is string => typeof entry === "string"));
}

interface PaletteState {
  /** The command palette is open. */
  open: boolean;
  /** The overview of all shortcuts ("?") is open. */
  overview: boolean;
  /** Keys of the commands used last, newest first (shown on top). */
  recent: readonly string[];
  openPalette: () => void;
  closePalette: () => void;
  openOverview: () => void;
  closeOverview: () => void;
  noteUsed: (key: string) => void;
  /** The core could not store the recent list; the previous list applies again. */
  saveError: CoreError | null;
  /** What the core holds (load, another window). */
  applyStored: (recent: readonly string[]) => void;
}

/** Stage 8: command palette and shortcut overview. */
export const usePaletteStore = create<PaletteState>()((set, get) => ({
  open: false,
  overview: false,
  recent: [],
  saveError: null,
  applyStored: (recent) => {
    set({ recent });
  },
  openPalette: () => {
    set({ open: true, overview: false });
  },
  closePalette: () => {
    set({ open: false });
  },
  openOverview: () => {
    set({ overview: true, open: false });
  },
  closeOverview: () => {
    set({ overview: false });
  },
  noteUsed: (key) => {
    const recent = cleanRecent([key, ...get().recent.filter((entry) => entry !== key)]);
    set({ recent, saveError: null });
    saveUiPreferences({ recentCommands: recent }).catch((error: unknown) => {
      set({ saveError: error as CoreError });
    });
  },
}));
