/**
 * The web view's local storage of this device. Stage 8 kept shortcut changes, the layout and the
 * recent palette commands here until the core could store them (decision 0025); since then they
 * live in the core and this module only serves the one-time migration of those old values
 * (`features/keyboard/migrateLocal.ts`; the AI panel's open state moved there with decision 0028
 * too, the accent theme of stage 16 with decision 0036). Nothing secret and no mail data goes here, and everything read back is
 * validated, because the storage is outside the app's control.
 */
const PREFIX = "vice-mail.ui.";

export type LocalKey =
  | "shortcuts"
  | "paletteRecent"
  | `layout.${string}`
  // Stage 16: the accent theme of the per-device fallback, read once by `migrateLocalAccent`
  // since the core stores it (decision 0036).
  | "accentTheme";

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
