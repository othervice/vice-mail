import { create } from "zustand";
import { saveUiPreferences, saveWindowLayout } from "../features/keyboard/uiPreferences";
import type { CoreError, CoreWindowLayout } from "../services/ipc";

/**
 * Where the reading pane opens (docs/DESIGN.md, docs/SPEC.md): right of the list (default),
 * below it, or in place of the list. "In its own window" is a command on a message instead
 * (`openInWindow`), not a position.
 */
export const READING_PANE_POSITIONS = ["right", "bottom", "replace"] as const;
export type ReadingPanePosition = (typeof READING_PANE_POSITIONS)[number];

/** Row height of the list: 44 px comfortable, 34 px compact (docs/DESIGN.md, "Zeile"). */
export const DENSITIES = ["comfortable", "compact"] as const;
export type Density = (typeof DENSITIES)[number];

/**
 * Text size in percent of the system's (docs/stages/08: 80 to 150 percent). Everything is in rem.
 * The core takes any whole percent in that range; the UI offers these and shows other values
 * (from another version) at the nearest one.
 */
export const TEXT_SIZES = [80, 90, 100, 115, 130, 150] as const;
export type TextSize = (typeof TEXT_SIZES)[number];

export function nearestTextSize(value: number): TextSize {
  let best: TextSize = 100;
  for (const size of TEXT_SIZES) if (Math.abs(size - value) < Math.abs(best - value)) best = size;
  return best;
}

export interface WindowLayout {
  readingPane: ReadingPanePosition;
  density: Density;
  /** From the UI preferences: it applies to every window. */
  textSize: TextSize;
  /** The rail unfolded into the labelled sidebar. */
  sidebarExpanded: boolean;
}

export const DEFAULT_LAYOUT: WindowLayout = { readingPane: "right", density: "comfortable", textSize: 100, sidebarExpanded: false };

function pick<T extends string | number>(value: unknown, allowed: readonly T[], fallback: T): T {
  return allowed.find((candidate) => candidate === value) ?? fallback;
}

/** A layout as the local fallback of an earlier version stored it (migration only). */
export function parseStoredLayout(raw: unknown): WindowLayout {
  const value = typeof raw === "object" && raw !== null ? (raw as Record<string, unknown>) : {};
  return {
    readingPane: pick(value["readingPane"], READING_PANE_POSITIONS, DEFAULT_LAYOUT.readingPane),
    density: pick(value["density"], DENSITIES, DEFAULT_LAYOUT.density),
    textSize: pick(value["textSize"], TEXT_SIZES, DEFAULT_LAYOUT.textSize),
    sidebarExpanded: value["sidebarExpanded"] === true,
  };
}

interface LayoutState extends WindowLayout {
  /** The core refused the last change; the previous value applies again. */
  saveError: CoreError | null;
  setLayout: (change: Partial<WindowLayout>) => void;
  /** The core's layout of this window (load, rollback). */
  applyStoredLayout: (layout: CoreWindowLayout) => void;
  /** The core's text size (load, another window or device). */
  applyStoredTextSize: (textSize: number) => void;
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

const windowPart = (layout: WindowLayout): CoreWindowLayout => ({
  readingPane: layout.readingPane,
  density: layout.density,
  sidebarExpanded: layout.sidebarExpanded,
});

/**
 * Layout of this window (stage 8): reading pane, density and sidebar are stored per kind of
 * window in the core (`getWindowLayout`), the text size with the UI preferences. A change
 * applies at once and is then saved; the sidebar state itself is in the UI store.
 */
export const useLayoutStore = create<LayoutState>()((set, get) => {
  applyToDocument(DEFAULT_LAYOUT);
  const failed = (error: unknown) => {
    set({ saveError: error as CoreError });
  };
  return {
    ...DEFAULT_LAYOUT,
    saveError: null,
    setLayout: (change) => {
      const next = { ...get(), ...change };
      applyToDocument(next);
      set({ ...change, saveError: null });
      if (change.textSize !== undefined) saveUiPreferences({ textSize: change.textSize }).catch(failed);
      if (change.readingPane !== undefined || change.density !== undefined || change.sidebarExpanded !== undefined) {
        saveWindowLayout(windowPart(next)).catch(failed);
      }
    },
    applyStoredLayout: (layout) => {
      const next = {
        readingPane: pick(layout.readingPane, READING_PANE_POSITIONS, DEFAULT_LAYOUT.readingPane),
        density: pick(layout.density, DENSITIES, DEFAULT_LAYOUT.density),
        sidebarExpanded: layout.sidebarExpanded,
      };
      applyToDocument({ density: next.density, textSize: get().textSize });
      set(next);
    },
    applyStoredTextSize: (value) => {
      const textSize = nearestTextSize(value);
      applyToDocument({ density: get().density, textSize });
      set({ textSize });
    },
  };
});
