import { create } from "zustand";
import type { Shortcut, ShortcutPlatform } from "../features/actions/shortcuts";
import {
  boundShortcutsFrom,
  checkShortcut,
  defaultShortcuts,
  parseOverrides,
  sameShortcut,
  type CommandId,
  type ShortcutOverrides,
  type ShortcutProblem,
} from "../features/keyboard/bindings";
import { readLocal, writeLocal } from "../features/keyboard/localFallback";

type Overrides = Record<ShortcutPlatform, ShortcutOverrides>;

interface ShortcutState {
  /** The user's changes per platform (a synced setting would carry both). */
  overrides: Overrides;
  /** The last change could not be stored: it lasts until the app quits. */
  saveFailed: boolean;
  /** Adds a shortcut, unless it is taken: then nothing changes and the problem comes back. */
  addShortcut: (platform: ShortcutPlatform, id: CommandId, shortcut: Shortcut) => ShortcutProblem | null;
  removeShortcut: (platform: ShortcutPlatform, id: CommandId, shortcut: Shortcut) => void;
  /** Back to the standard, unless another command has one of its keys now (the problem comes back). */
  resetCommand: (platform: ShortcutPlatform, id: CommandId) => ShortcutProblem | null;
  resetAll: (platform: ShortcutPlatform) => void;
}

/** What is stored (stage 8: local fallback until the core has a command for it). */
export function loadStoredOverrides(): Overrides {
  const raw = readLocal("shortcuts");
  const value = typeof raw === "object" && raw !== null ? (raw as Record<string, unknown>) : {};
  return { macos: parseOverrides(value["macos"]), windows: parseOverrides(value["windows"]) };
}

/**
 * The user's shortcut assignment (Settings → Keyboard). Every dispatcher reads it at the moment
 * of the key press (`boundShortcuts`), so a change takes effect at once.
 */
export const useShortcutStore = create<ShortcutState>()((set, get) => {
  const store = (overrides: Overrides) => {
    set({ overrides, saveFailed: !writeLocal("shortcuts", overrides) });
  };
  const change = (platform: ShortcutPlatform, next: ShortcutOverrides) => {
    store({ ...get().overrides, [platform]: next });
  };
  return {
    overrides: loadStoredOverrides(),
    saveFailed: false,
    addShortcut: (platform, id, shortcut) => {
      const current = get().overrides[platform];
      const problem = checkShortcut(id, shortcut, platform, current);
      if (problem) return problem;
      change(platform, { ...current, [id]: [...boundShortcutsFrom(current, id, platform), shortcut] });
      return null;
    },
    removeShortcut: (platform, id, shortcut) => {
      const current = get().overrides[platform];
      const kept = boundShortcutsFrom(current, id, platform).filter((s) => !sameShortcut(s, shortcut));
      change(platform, { ...current, [id]: kept });
    },
    resetCommand: (platform, id) => {
      const next = { ...get().overrides[platform] };
      delete next[id];
      // Another command may have taken one of the standard keys meanwhile.
      for (const shortcut of defaultShortcuts(id, platform)) {
        const problem = checkShortcut(id, shortcut, platform, { ...next, [id]: [] });
        if (problem?.kind === "conflict") return problem;
      }
      change(platform, next);
      return null;
    },
    resetAll: (platform) => {
      change(platform, {});
    },
  };
});

/** The shortcuts of a command now, defaults or the user's. For key handlers (no re-render needed). */
export function boundShortcuts(id: CommandId, platform: ShortcutPlatform): readonly Shortcut[] {
  return boundShortcutsFrom(useShortcutStore.getState().overrides[platform], id, platform);
}

/** The same for rendering: tooltips and hints update when the assignment changes. */
export function useBoundShortcuts(platform: ShortcutPlatform): (id: CommandId) => readonly Shortcut[] {
  const overrides = useShortcutStore((state) => state.overrides[platform]);
  return (id) => boundShortcutsFrom(overrides, id, platform);
}
