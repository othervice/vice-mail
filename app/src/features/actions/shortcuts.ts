import type { Platform } from "../../services/ipc";

/**
 * A keyboard shortcut, matched by the character it produces (`event.key`), so it follows the
 * keyboard layout: "#" is "#" on a German and on a US keyboard, wherever the key sits.
 *
 * - Letters compare case-insensitively (Caps Lock does not matter); `shift` must match exactly,
 *   so "I" with `shift` is Shift+I and "e" is E without Shift.
 * - Other characters ("#", "!") ignore Shift: they need it on some layouts and not on others.
 * - `mod` is Command on macOS and Control elsewhere. Without `mod`, neither may be held, and Alt
 *   never (AltGr on Windows reports Control and Alt).
 *
 * The final assignment and its settings come in stage 8 (docs/stages/08-...).
 */
export interface Shortcut {
  key: string;
  shift?: boolean;
  mod?: boolean;
}

export type ShortcutPlatform = "macos" | "windows";

export function shortcutPlatform(platform: Platform | undefined): ShortcutPlatform {
  return platform === "macos" ? "macos" : "windows";
}

const isLetter = (key: string) => key.length === 1 && key.toLowerCase() !== key.toUpperCase();

export function matchesShortcut(
  event: Pick<KeyboardEvent, "key" | "shiftKey" | "metaKey" | "ctrlKey" | "altKey">,
  shortcut: Shortcut,
  platform: ShortcutPlatform,
): boolean {
  if (event.altKey) return false;
  const mod = platform === "macos" ? event.metaKey : event.ctrlKey;
  const otherMod = platform === "macos" ? event.ctrlKey : event.metaKey;
  if (otherMod || mod !== Boolean(shortcut.mod)) return false;
  if (isLetter(shortcut.key)) {
    return event.key.toLowerCase() === shortcut.key.toLowerCase() && event.shiftKey === Boolean(shortcut.shift);
  }
  if (shortcut.key.length > 1 && event.shiftKey !== Boolean(shortcut.shift)) return false;
  return event.key === shortcut.key;
}

const MAC_KEYS: Record<string, string> = { Backspace: "⌫", Delete: "⌦", " ": "Space", Escape: "Esc" };
const OTHER_KEYS: Record<string, string> = { Backspace: "Backspace", Delete: "Del", " ": "Space", Escape: "Esc" };

/** How a shortcut is written: "⌘A", "⇧I" on macOS; "Ctrl+A", "Shift+I" elsewhere. */
export function formatShortcut(shortcut: Shortcut, platform: ShortcutPlatform): string {
  const names = platform === "macos" ? MAC_KEYS : OTHER_KEYS;
  const key = names[shortcut.key] ?? (isLetter(shortcut.key) ? shortcut.key.toUpperCase() : shortcut.key);
  if (platform === "macos") return `${shortcut.mod ? "⌘" : ""}${shortcut.shift ? "⇧" : ""}${key}`;
  return [shortcut.mod && "Ctrl", shortcut.shift && "Shift", key].filter(Boolean).join("+");
}

/** The `aria-keyshortcuts` value (WAI-ARIA syntax): "Meta+A", "Shift+I", "E". */
export function ariaShortcut(shortcut: Shortcut, platform: ShortcutPlatform): string {
  const key = shortcut.key === " " ? "Space" : isLetter(shortcut.key) ? shortcut.key.toUpperCase() : shortcut.key;
  return [shortcut.mod && (platform === "macos" ? "Meta" : "Control"), shortcut.shift && "Shift", key]
    .filter(Boolean)
    .join("+");
}

/** Focus in a field where typing means text: shortcuts stay out of the way. */
export function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) return true;
  if (target instanceof HTMLInputElement) {
    return !["button", "checkbox", "radio", "submit", "reset", "range", "color", "file"].includes(target.type);
  }
  return false;
}
