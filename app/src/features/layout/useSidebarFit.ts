import { useEffect } from "react";
import { useLayoutStore, type ReadingPanePosition } from "../../stores/layout";
import { useUiStore } from "../../stores/ui";

/**
 * Narrowest window (px at 100 % text size, column at its standard width) that keeps the column
 * next to the list: with the reading pane on the right the list needs its 24 rem and the pane
 * about 30 rem beside the 15 rem column; below or in place of the list only the list remains.
 */
export const SIDEBAR_FIT_MIN_WIDTH: Record<ReadingPanePosition, number> = { right: 1100, bottom: 880, replace: 880 };

/**
 * docs/DESIGN.md, "Seitenspalte": in a window too narrow for the column next to the list (and the
 * reading pane), the column folds to the rail by itself and opens over the content instead; it
 * comes back once the window is wide enough. The limit grows with the text size (not with the
 * column's own width, so dragging its edge never folds it). Never a horizontal scroll bar.
 */
export function useSidebarFit(): void {
  const readingPane = useLayoutStore((state) => state.readingPane);
  const textSize = useLayoutStore((state) => state.textSize);
  const setNarrow = useUiStore((state) => state.setSidebarNarrow);
  useEffect(() => {
    const min = Math.round((SIDEBAR_FIT_MIN_WIDTH[readingPane] * textSize) / 100);
    const media = window.matchMedia(`(max-width: ${String(min - 1)}px)`);
    const apply = () => {
      setNarrow(media.matches);
    };
    apply();
    media.addEventListener("change", apply);
    return () => {
      media.removeEventListener("change", apply);
    };
  }, [readingPane, textSize, setNarrow]);
}
