import {
  ACTION_IDS,
  ACTIONS,
  APP_COMMAND_IDS,
  APP_COMMANDS,
  type ActionId,
  type AppCommandId,
} from "../actions/registry";
import type { Shortcut, ShortcutPlatform } from "../actions/shortcuts";

/**
 * Stage 8: the final shortcut assignment and the user's changes to it. The defaults live in the
 * action directory (`actions/registry.ts`); this module only adds the user's overrides and the
 * rules for what may be assigned.
 *
 * Keyboard layouts (docs/ARCHITECTURE.md, "Tastaturkürzel"): shortcuts that stand for a
 * character ("e", "#", "?", "/") match the character the key produces (`event.key`), so they
 * follow the layout. Nothing in the app is bound to a key position (`event.code`); if a shortcut
 * ever needs one (e.g. "the key left of 1"), it gets its own field instead of reusing `key`.
 */
export type CommandId = ActionId | AppCommandId;

export const COMMAND_IDS: readonly CommandId[] = [...ACTION_IDS, ...APP_COMMAND_IDS];

const ACTION_ID_SET: ReadonlySet<string> = new Set(ACTION_IDS);

export function isActionId(id: CommandId): id is ActionId {
  return ACTION_ID_SET.has(id);
}

export function isCommandId(value: unknown): value is CommandId {
  return typeof value === "string" && (COMMAND_IDS as readonly string[]).includes(value);
}

/** The shortcuts the action directory assigns. */
export function defaultShortcuts(id: CommandId, platform: ShortcutPlatform): readonly Shortcut[] {
  return isActionId(id) ? ACTIONS[id].shortcuts[platform] : APP_COMMANDS[id].shortcuts[platform];
}

/** The user's changes: the whole list of shortcuts of a command, per platform. */
export type ShortcutOverrides = Partial<Record<CommandId, readonly Shortcut[]>>;

export function boundShortcutsFrom(
  overrides: ShortcutOverrides,
  id: CommandId,
  platform: ShortcutPlatform,
): readonly Shortcut[] {
  return overrides[id] ?? defaultShortcuts(id, platform);
}

/**
 * Commands that are never available at the same time and so may share a key: "Delete" outside
 * the trash and "Delete permanently" in it, "Junk" and "Not junk".
 */
const SHARED_KEYS: readonly (readonly CommandId[])[] = [
  ["delete", "deletePermanently"],
  ["spam", "notSpam"],
];

export function mayShareKeys(a: CommandId, b: CommandId): boolean {
  return SHARED_KEYS.some((group) => group.includes(a) && group.includes(b));
}

const isLetter = (key: string) => key.length === 1 && key.toLowerCase() !== key.toUpperCase();

/**
 * One string per distinct shortcut, by the rules of `matchesShortcut`: letters ignore case but
 * not Shift; other single characters ignore Shift (it depends on the layout); named keys
 * ("Delete", "Enter") take Shift into account.
 */
export function shortcutIdentity(shortcut: Shortcut): string {
  const mod = shortcut.mod ? "mod+" : "";
  if (isLetter(shortcut.key)) return `${mod}${shortcut.shift ? "shift+" : ""}${shortcut.key.toLowerCase()}`;
  if (shortcut.key.length === 1) return `${mod}${shortcut.key}`;
  return `${mod}${shortcut.shift ? "shift+" : ""}${shortcut.key}`;
}

export function sameShortcut(a: Shortcut, b: Shortcut): boolean {
  return shortcutIdentity(a) === shortcutIdentity(b);
}

const plain = (key: string): Shortcut => ({ key });
const withMod = (key: string): Shortcut => ({ key, mod: true });

/**
 * Keys the app uses for moving around and that cannot be reassigned: the list (arrows, j/k,
 * Home/End, Page Up/Down), opening and closing (Enter, Escape), Tab for the focus order.
 */
const MOVING_KEYS = ["Enter", "Escape", "Tab", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End", "PageUp", "PageDown"];

export const FIXED_KEYS: readonly Shortcut[] = [
  ...[...MOVING_KEYS, "j", "k"].map(plain),
  // With Shift they extend the selection (list) or go backwards (Tab).
  ...MOVING_KEYS.map((key): Shortcut => ({ key, shift: true })),
];

/**
 * Shortcuts of the app outside the directory: printing (features/extras, Cmd/Ctrl+P) and sending
 * in the composer (Cmd/Ctrl+Enter). Not changeable, so not assignable either.
 */
const APP_KEYS: readonly Shortcut[] = [withMod("p"), withMod("Enter")];

/**
 * What the operating system or every text field claims (docs/stages/08: no shortcuts the
 * system uses): quit, close, hide, minimise, settings, Spotlight, window switching; copy,
 * paste and cut everywhere.
 */
const SYSTEM_KEYS: Record<ShortcutPlatform, readonly Shortcut[]> = {
  macos: ["q", "w", "h", "m", ",", " ", "`", "Tab", "c", "v", "x"].map(withMod),
  windows: ["Escape", "Tab", "c", "v", "x"].map(withMod),
};

export type ShortcutProblem =
  /** The system or every text field uses it. */
  | { kind: "system" }
  /** One of the fixed keys for moving around. */
  | { kind: "fixed" }
  /** Used by the app outside the directory (printing, sending). */
  | { kind: "app" }
  /** Another command has it already. */
  | { kind: "conflict"; other: CommandId }
  /** This command has it already. */
  | { kind: "duplicate" };

/** Whether `shortcut` may be added to `id`, given the current assignment. */
export function checkShortcut(
  id: CommandId,
  shortcut: Shortcut,
  platform: ShortcutPlatform,
  overrides: ShortcutOverrides,
): ShortcutProblem | null {
  if (SYSTEM_KEYS[platform].some((taken) => sameShortcut(taken, shortcut))) return { kind: "system" };
  if (FIXED_KEYS.some((taken) => sameShortcut(taken, shortcut))) return { kind: "fixed" };
  if (APP_KEYS.some((taken) => sameShortcut(taken, shortcut))) return { kind: "app" };
  if (boundShortcutsFrom(overrides, id, platform).some((own) => sameShortcut(own, shortcut))) return { kind: "duplicate" };
  for (const other of COMMAND_IDS) {
    if (other === id || mayShareKeys(id, other)) continue;
    if (boundShortcutsFrom(overrides, other, platform).some((taken) => sameShortcut(taken, shortcut))) {
      return { kind: "conflict", other };
    }
  }
  return null;
}

/** Every pair of commands that share a shortcut although they may not (empty when all is well). */
export function findConflicts(
  platform: ShortcutPlatform,
  overrides: ShortcutOverrides,
): { a: CommandId; b: CommandId; shortcut: Shortcut }[] {
  const seen = new Map<string, CommandId[]>();
  const conflicts: { a: CommandId; b: CommandId; shortcut: Shortcut }[] = [];
  for (const id of COMMAND_IDS) {
    for (const shortcut of boundShortcutsFrom(overrides, id, platform)) {
      const key = shortcutIdentity(shortcut);
      const owners = seen.get(key) ?? [];
      for (const owner of owners) {
        if (owner !== id && !mayShareKeys(owner, id)) conflicts.push({ a: owner, b: id, shortcut });
      }
      seen.set(key, [...owners, id]);
    }
  }
  return conflicts;
}

export type RecordedKey =
  | { kind: "shortcut"; shortcut: Shortcut }
  /** Only a modifier so far: keep listening. */
  | { kind: "pending" }
  /** Option/Alt types characters on many layouts (and AltGr reports Control and Alt). */
  | { kind: "alt" }
  /** Control on macOS, the Windows key on Windows: not part of the scheme. */
  | { kind: "otherModifier" }
  /** A dead key or an unidentified one: no character to match. */
  | { kind: "unusable" };

const MODIFIER_KEYS = new Set(["Shift", "Control", "Meta", "Alt", "AltGraph", "CapsLock", "Fn", "FnLock", "OS", "Hyper", "Super"]);

/** Turns a key press in the "record a shortcut" field into a shortcut, by the same rules as matching. */
export function shortcutFromEvent(
  event: Pick<KeyboardEvent, "key" | "shiftKey" | "metaKey" | "ctrlKey" | "altKey">,
  platform: ShortcutPlatform,
): RecordedKey {
  if (MODIFIER_KEYS.has(event.key)) return { kind: "pending" };
  if (event.key === "Dead" || event.key === "Unidentified" || event.key === "Process" || event.key === "") {
    return { kind: "unusable" };
  }
  if (event.altKey) return { kind: "alt" };
  const mod = platform === "macos" ? event.metaKey : event.ctrlKey;
  const otherMod = platform === "macos" ? event.ctrlKey : event.metaKey;
  if (otherMod) return { kind: "otherModifier" };
  const key = event.key;
  if (isLetter(key)) {
    // Letters are stored as the registry writes them: "e", or "I" with Shift.
    return {
      kind: "shortcut",
      shortcut: { key: event.shiftKey ? key.toUpperCase() : key.toLowerCase(), ...(event.shiftKey && { shift: true }), ...(mod && { mod: true }) },
    };
  }
  // Other characters ignore Shift (it depends on the layout); named keys keep it.
  const shift = key.length > 1 && event.shiftKey;
  return { kind: "shortcut", shortcut: { key, ...(shift && { shift: true }), ...(mod && { mod: true }) } };
}

/** Untrusted stored data back into overrides: unknown commands and malformed entries are dropped. */
export function parseOverrides(raw: unknown): ShortcutOverrides {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return {};
  const result: ShortcutOverrides = {};
  for (const [id, list] of Object.entries(raw as Record<string, unknown>)) {
    if (!isCommandId(id) || !Array.isArray(list)) continue;
    const shortcuts: Shortcut[] = [];
    for (const entry of list as unknown[]) {
      if (typeof entry !== "object" || entry === null) continue;
      const { key, shift, mod } = entry as Record<string, unknown>;
      if (typeof key !== "string" || key.length === 0 || key.length > 20) continue;
      shortcuts.push({ key, ...(shift === true && { shift: true }), ...(mod === true && { mod: true }) });
    }
    result[id] = shortcuts;
  }
  return result;
}
