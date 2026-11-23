import { useEffect, useEffectEvent } from "react";
import { useSearchStore } from "../../stores/search";
import { boundShortcuts } from "../../stores/shortcuts";
import { isEditableTarget, matchesShortcut, type ShortcutPlatform } from "../actions/shortcuts";

/**
 * The search shortcut from the action directory (`APP_COMMANDS.search`, as the user assigned
 * it): opens the search field and puts the focus into it, from anywhere in the mail view except
 * while typing elsewhere.
 */
export function useSearchShortcut(platform: ShortcutPlatform): void {
  const openSearch = useSearchStore((state) => state.openSearch);
  const onKey = useEffectEvent((event: KeyboardEvent) => {
    if (event.defaultPrevented || event.repeat) return;
    const shortcuts = boundShortcuts("search", platform);
    const match = shortcuts.find((shortcut) => matchesShortcut(event, shortcut, platform));
    if (!match) return;
    // A plain key ("/") is text in a field; Cmd/Ctrl+F works from fields too.
    if (!match.mod && isEditableTarget(event.target)) return;
    event.preventDefault();
    openSearch();
  });
  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      onKey(event);
    };
    window.addEventListener("keydown", listener);
    return () => {
      window.removeEventListener("keydown", listener);
    };
  }, []);
}
