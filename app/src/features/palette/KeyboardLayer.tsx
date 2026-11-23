import { useEffect, useEffectEvent } from "react";
import { usePaletteStore } from "../../stores/palette";
import { boundShortcuts } from "../../stores/shortcuts";
import { isEditableTarget, matchesShortcut, shortcutPlatform } from "../actions/shortcuts";
import { useAppInfo } from "../app-info/queries";
import { ShortcutOverview } from "../keyboard/ShortcutOverview";
import { useLayoutEffects } from "../layout/useLayoutEffects";
import { CommandPalette } from "./CommandPalette";

/**
 * Stage 8, mounted once in the frame: the command palette (Cmd/Ctrl+K, also from text fields
 * like Cmd/Ctrl+F) and the shortcut overview ("?", not while typing), with the user's
 * assignment; the layout of the window.
 */
export function KeyboardLayer() {
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const openPalette = usePaletteStore((state) => state.openPalette);
  const openOverview = usePaletteStore((state) => state.openOverview);
  useLayoutEffects();

  const onKey = useEffectEvent((event: KeyboardEvent) => {
    if (event.defaultPrevented || event.repeat) return;
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
