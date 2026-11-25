/** Geometry of the message list in CSS pixels, from the tokens in styles/tokens.css. */
export interface ListMetrics {
  row: number;
  twoLineRow: number;
  section: number;
  /** Stage 17: a message of an unfolded conversation (one line in every width). */
  childRow: number;
  /** List width below which rows get two lines (docs/DESIGN.md: 720 px). */
  twoLineBelow: number;
  /** Space above the first and below the last row. */
  padding: number;
}

/** The token values at the default text size; used where no stylesheet is loaded (tests). */
const DEFAULTS: ListMetrics = { row: 44, twoLineRow: 76, section: 32, childRow: 36, twoLineBelow: 720, padding: 8 };

function toPixels(value: string, rootPixels: number): number | null {
  const number = Number.parseFloat(value);
  if (!Number.isFinite(number) || number <= 0) return null;
  if (value.endsWith("rem")) return number * rootPixels;
  if (value.endsWith("px")) return number;
  return null;
}

/** A length token (rem or px) of the root in CSS pixels at the current text size, else `fallback`. */
export function readTokenPixels(name: string, fallback: number): number {
  const style = getComputedStyle(document.documentElement);
  const rootPixels = Number.parseFloat(style.fontSize) || 16;
  return toPixels(style.getPropertyValue(name).trim(), rootPixels) ?? fallback;
}

/**
 * Reads the row heights the virtualizer needs from the design tokens, so CSS and the list
 * geometry cannot drift apart. Rem values follow the root font size (text scaling).
 */
export function readListMetrics(): ListMetrics {
  const read = readTokenPixels;
  return {
    row: read("--ui-row-height", DEFAULTS.row),
    twoLineRow: read("--ui-row-height-two-line", DEFAULTS.twoLineRow),
    section: read("--ui-section-height", DEFAULTS.section),
    childRow: read("--ui-child-row-height", DEFAULTS.childRow),
    twoLineBelow: read("--ui-list-two-line-below", DEFAULTS.twoLineBelow),
    padding: read("--ui-space-2", DEFAULTS.padding),
  };
}
