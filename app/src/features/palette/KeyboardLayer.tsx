import { useEffect, useEffectEvent } from "react";
import { usePaletteStore } from "../../stores/palette";
import { boundShortcuts } from "../../stores/shortcuts";
import { useUiStore } from "../../stores/ui";
import { VIEW_COMMANDS, type ViewCommandId } from "../actions/registry";
import { isEditableTarget, matchesShortcut, shortcutPlatform } from "../actions/shortcuts";
import { useAppInfo } from "../app-info/queries";
import { ShortcutOverview } from "../keyboard/ShortcutOverview";
import { useStoredSidebar } from "../layout/useStoredSidebar";
import { CommandPalette } from "./CommandPalette";

/**
 * Stage 8, mounted once in the frame: the command palette (Cmd/Ctrl+K, also from text fields
 * like Cmd/Ctrl+F), the shortcut overview ("?", not while typing) and the views of the rail
 * (Cmd/Ctrl+1 to 8), with the user's assignment; the sidebar state of the window.
 */
export function KeyboardLayer() {
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const openPalette = usePaletteStore((state) => state.openPalette);
  const openOverview = usePaletteStore((state) => state.openOverview);
  const showMailView = useUiStore((state) => state.showMailView);
  useStoredSidebar();

  const onKey = useEffectEvent((event: KeyboardEvent) => {
    if (event.defaultPrevented || event.repeat) return;
    // Not from inside a dialog or picker (the confirmation dialog makes the rest inert).
    if (event.target instanceof Element && event.target.closest('[role="dialog"], [role="alertdialog"]')) return;
    const palette = boundShortcuts("palette", platform).find((s) => matchesShortcut(event, s, platform));
    if (palette && (palette.mod || !isEditableTarget(event.target))) {
      event.preventDefault();
      openPalette();
      return;
    }
    const help = boundShortcuts("shortcutHelp", platform).find((s) => matchesShortcut(event, s, platform));
    if (help && (help.mod || !isEditableTarget(event.target))) {
      event.preventDefault();
      openOverview();
      return;
    }
    for (const id of Object.keys(VIEW_COMMANDS) as ViewCommandId[]) {
      const match = boundShortcuts(id, platform).find((s) => matchesShortcut(event, s, platform));
      if (!match || (!match.mod && isEditableTarget(event.target))) continue;
      event.preventDefault();
      showMailView(VIEW_COMMANDS[id]);
      return;
    }
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

  return (
    <>
      <CommandPalette platform={platform} />
      <ShortcutOverview platform={platform} />
    </>
  );
}
