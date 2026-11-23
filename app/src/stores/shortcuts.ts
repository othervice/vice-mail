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
import { saveUiPreferences, type PlatformOverrides } from "../features/keyboard/uiPreferences";
import type { CoreError } from "../services/ipc";

type Overrides = PlatformOverrides;

interface ShortcutState {
  /** The user's changes per platform (the core syncs both between devices). */
  overrides: Overrides;
  /** The core refused the last change; the previous assignment applies again. */
  saveError: CoreError | null;
  /** What the core holds now (load, save, another window or device): replaces the assignment. */
  applyStored: (overrides: Overrides) => void;
  /** Adds a shortcut, unless it is taken: then nothing changes and the problem comes back. */
  addShortcut: (platform: ShortcutPlatform, id: CommandId, shortcut: Shortcut) => ShortcutProblem | null;
  removeShortcut: (platform: ShortcutPlatform, id: CommandId, shortcut: Shortcut) => void;
  /** Back to the standard, unless another command has one of its keys now (the problem comes back). */
  resetCommand: (platform: ShortcutPlatform, id: CommandId) => ShortcutProblem | null;
  resetAll: (platform: ShortcutPlatform) => void;
}

/** Shortcut changes as the local fallback of an earlier version stored them (migration only). */
export function parseStoredOverrides(raw: unknown): Overrides {
  const value = typeof raw === "object" && raw !== null ? (raw as Record<string, unknown>) : {};
  return { macos: parseOverrides(value["macos"]), windows: parseOverrides(value["windows"]) };
}

const NO_OVERRIDES: Overrides = { macos: {}, windows: {} };

/**
 * The user's shortcut assignment (Settings → Keyboard). Every dispatcher reads it at the moment
 * of the key press (`boundShortcuts`), so a change takes effect at once.
 */
export const useShortcutStore = create<ShortcutState>()((set, get) => {
  // At once in this window (a key press uses it right away), then in the core. A refusal puts
  // the core's value back (through `applyStored`) and keeps the error for the settings.
  const store = (overrides: Overrides) => {
    set({ overrides, saveError: null });
    saveUiPreferences({ overrides }).catch((error: unknown) => {
      set({ saveError: error as CoreError });
    });
  };
  const change = (platform: ShortcutPlatform, next: ShortcutOverrides) => {
    store({ ...get().overrides, [platform]: next });
  };
  return {
    overrides: NO_OVERRIDES,
    saveError: null,
    applyStored: (overrides) => {
      set({ overrides });
    },
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
