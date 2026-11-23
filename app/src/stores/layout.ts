import { create } from "zustand";
import { readLocal, writeLocal } from "../features/keyboard/localFallback";

/**
 * Where the reading pane opens (docs/DESIGN.md, docs/SPEC.md): right of the list (default),
 * below it, or in place of the list. A pane in its own window needs a core command first.
 */
export const READING_PANE_POSITIONS = ["right", "bottom", "replace"] as const;
export type ReadingPanePosition = (typeof READING_PANE_POSITIONS)[number];

/** Row height of the list: 44 px comfortable, 34 px compact (docs/DESIGN.md, "Zeile"). */
export const DENSITIES = ["comfortable", "compact"] as const;
export type Density = (typeof DENSITIES)[number];

/** Text size in percent of the system's (docs/stages/08: 80 to 150 percent). Everything is in rem. */
export const TEXT_SIZES = [80, 90, 100, 115, 130, 150] as const;
export type TextSize = (typeof TEXT_SIZES)[number];

export interface WindowLayout {
  readingPane: ReadingPanePosition;
  density: Density;
  textSize: TextSize;
  /** The rail unfolded into the labelled sidebar. */
  sidebarExpanded: boolean;
}

export const DEFAULT_LAYOUT: WindowLayout = { readingPane: "right", density: "comfortable", textSize: 100, sidebarExpanded: false };

/** The app has one window so far; a second window (stage 8 "own window") gets its own label. */
export const MAIN_WINDOW = "main";

function pick<T extends string | number>(value: unknown, allowed: readonly T[], fallback: T): T {
  return allowed.find((candidate) => candidate === value) ?? fallback;
}

export function loadLayout(window: string): WindowLayout {
  const raw = readLocal(`layout.${window}`);
  const value = typeof raw === "object" && raw !== null ? (raw as Record<string, unknown>) : {};
  return {
    readingPane: pick(value["readingPane"], READING_PANE_POSITIONS, DEFAULT_LAYOUT.readingPane),
    density: pick(value["density"], DENSITIES, DEFAULT_LAYOUT.density),
    textSize: pick(value["textSize"], TEXT_SIZES, DEFAULT_LAYOUT.textSize),
    sidebarExpanded: value["sidebarExpanded"] === true,
  };
}

interface LayoutState extends WindowLayout {
  window: string;
  saveFailed: boolean;
  setLayout: (change: Partial<WindowLayout>) => void;
}

/**
 * tokens.css switches the row heights on `data-density` and the root font size on
 * `data-text-size`. Set before the store changes, so the list reads the new heights when it
 * renders for the change.
 */
function applyToDocument(layout: Pick<WindowLayout, "density" | "textSize">): void {
  const root = document.documentElement;
  root.dataset["density"] = layout.density;
  if (layout.textSize === 100) delete root.dataset["textSize"];
  else root.dataset["textSize"] = String(layout.textSize);
}

/** Layout of this window (stage 8), stored per window. The sidebar state itself is in the UI store. */
export const useLayoutStore = create<LayoutState>()((set, get) => {
  const initial = loadLayout(MAIN_WINDOW);
  applyToDocument(initial);
  return {
    window: MAIN_WINDOW,
    ...initial,
    saveFailed: false,
    setLayout: (change) => {
      const { window, readingPane, density, textSize, sidebarExpanded } = { ...get(), ...change };
      const saved = writeLocal(`layout.${window}`, { readingPane, density, textSize, sidebarExpanded });
      applyToDocument({ density, textSize });
      set({ ...change, saveFailed: !saved });
    },
  };
});
