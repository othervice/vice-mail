import { create } from "zustand";
import { readLocal, writeLocal } from "../features/keyboard/localFallback";

const MAX_RECENT = 5;

function loadRecent(): string[] {
  const raw = readLocal("paletteRecent");
  if (!Array.isArray(raw)) return [];
  return raw.filter((entry): entry is string => typeof entry === "string" && entry.length < 200).slice(0, MAX_RECENT);
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
}

/** Stage 8: command palette and shortcut overview. */
export const usePaletteStore = create<PaletteState>()((set) => ({
  open: false,
  overview: false,
  recent: loadRecent(),
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
    set((state) => {
      const recent = [key, ...state.recent.filter((entry) => entry !== key)].slice(0, MAX_RECENT);
      writeLocal("paletteRecent", recent);
      return { recent };
    });
  },
}));

export function reloadPaletteRecent(): void {
  usePaletteStore.setState({ recent: loadRecent() });
}
