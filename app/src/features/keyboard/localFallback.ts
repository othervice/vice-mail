/**
 * LOCAL FALLBACK, stage 8 (to be replaced): the core has no command yet for the user's shortcut
 * changes, the layout per window and the recently used palette commands. Until it has one
 * (requested in the stage 8 result: `get_ui_preferences` / `set_ui_preferences`), these few UI
 * values live in the web view's local storage of this device. Nothing secret and no mail data
 * goes here, and everything read back is validated, because the storage is outside the app's
 * control. When the commands exist, only this module changes.
 */
const PREFIX = "vice-mail.ui.";

export type LocalKey = "shortcuts" | "paletteRecent" | `layout.${string}`;

export function readLocal(key: LocalKey): unknown {
  try {
    const raw = window.localStorage.getItem(PREFIX + key);
    return raw === null ? null : (JSON.parse(raw) as unknown);
  } catch {
    // Storage switched off or a broken value: the defaults apply.
    return null;
  }
}

/** Returns false if the value could not be stored (the change then lasts until the app quits). */
export function writeLocal(key: LocalKey, value: unknown): boolean {
  try {
    window.localStorage.setItem(PREFIX + key, JSON.stringify(value));
    return true;
  } catch {
    return false;
  }
}

export function removeLocal(key: LocalKey): void {
  try {
    window.localStorage.removeItem(PREFIX + key);
  } catch {
    // Nothing stored that could be removed.
  }
}
