/** Geometry of the message list in CSS pixels, from the tokens in styles/tokens.css. */
export interface ListMetrics {
  row: number;
  twoLineRow: number;
  section: number;
  /** List width below which rows get two lines (docs/DESIGN.md: 720 px). */
  twoLineBelow: number;
  /** Space above the first and below the last row. */
  padding: number;
}

/** The token values at the default text size; used where no stylesheet is loaded (tests). */
const DEFAULTS: ListMetrics = { row: 44, twoLineRow: 76, section: 32, twoLineBelow: 720, padding: 8 };

function toPixels(value: string, rootPixels: number): number | null {
  const number = Number.parseFloat(value);
  if (!Number.isFinite(number) || number <= 0) return null;
  if (value.endsWith("rem")) return number * rootPixels;
  if (value.endsWith("px")) return number;
  return null;
}

/**
 * Reads the row heights the virtualizer needs from the design tokens, so CSS and the list
 * geometry cannot drift apart. Rem values follow the root font size (text scaling).
 */
export function readListMetrics(): ListMetrics {
  const root = document.documentElement;
  const style = getComputedStyle(root);
  const rootPixels = Number.parseFloat(style.fontSize) || 16;
  const read = (name: string, fallback: number) => toPixels(style.getPropertyValue(name).trim(), rootPixels) ?? fallback;
  return {
    row: read("--ui-row-height", DEFAULTS.row),
    twoLineRow: read("--ui-row-height-two-line", DEFAULTS.twoLineRow),
    section: read("--ui-section-height", DEFAULTS.section),
    twoLineBelow: read("--ui-list-two-line-below", DEFAULTS.twoLineBelow),
    padding: read("--ui-space-2", DEFAULTS.padding),
  };
}
