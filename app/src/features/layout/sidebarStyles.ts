import { cx } from "../../components/cx";

/**
 * Shared look of the accounts-and-folders column (docs/DESIGN.md, "Seitenspalte"; stage 23,
 * decision 0085, the approved draft): 28 px rows with 13 px text in the primary colour, a 16 px
 * icon (tertiary, or the account's colour), the count at the end. The entry shown right now sits
 * on the column's neutral selected fill (`rail-selected`), semibold, its icon in the accent where
 * it has no account colour; no bar at its start (fill, weight and icon tell it apart). Every tree
 * of the column uses these, so they read as one column.
 *
 * A row of a column tree; `extra` adds padding, indentation and drop states.
 */
export function sidebarRowClass(current: boolean, extra?: string | false): string {
  return cx(
    "relative flex h-(--ui-sidebar-row) min-w-0 items-center rounded-control text-13 text-primary transition-colors",
    current ? "bg-rail-selected font-semibold" : "hover:bg-rail-hover",
    extra,
  );
}

/**
 * The icon of a row: the accent while the row is the one shown, else tertiary. `tone` (an
 * account's colour, `accountText`) keeps its colour either way: it says whose folder it is.
 */
export function sidebarIconClass(current: boolean, tone?: string): string {
  return cx("size-(--ui-icon-small) shrink-0", tone ?? (current ? "text-accent" : "text-tertiary"));
}

/**
 * A section's label as the first row of its tree (11 px semibold, tertiary): "All accounts",
 * "Favourites", an account's name. 24 px, its text at the bottom, 8 px above the section's rows.
 */
export const SECTION_HEAD_CLASS =
  "group/section relative flex h-6 min-w-0 items-center gap-2 rounded-control px-2 text-11 font-semibold text-tertiary transition-colors hover:text-secondary";

/**
 * Start padding per level inside "More" and "More folders" (the token grid has no inline styles):
 * the first level 8 px like every row, then 16 px a level, flat from level 6. Padding instead of a
 * margin, so the fill spans the whole row.
 */
const INDENT = [
  "ps-2",
  "ps-6",
  "ps-10",
  "ps-[calc(var(--ui-space-1)*14)]",
  "ps-[calc(var(--ui-space-1)*18)]",
  "ps-[calc(var(--ui-space-1)*22)]",
] as const;

/** Indentation of a row at `level` (1 = none beyond the row's own 8 px). */
export function indentClass(level: number): string {
  return INDENT[Math.min(Math.max(level - 1, 0), INDENT.length - 1)] ?? "";
}

/** Where the arrow of a folder with subfolders sits: in the 16 px before its icon. */
const TOGGLE_START = [
  "start-0",
  "start-2",
  "start-6",
  "start-10",
  "start-[calc(var(--ui-space-1)*14)]",
  "start-[calc(var(--ui-space-1)*18)]",
] as const;

/** The arrow's place for a row indented with `indentClass(level)`. */
export function toggleStartClass(level: number): string {
  return TOGGLE_START[Math.min(Math.max(level - 1, 0), TOGGLE_START.length - 1)] ?? "";
}
