import { cx } from "../../components/cx";

/**
 * Shared look of the accounts-and-folders column (docs/DESIGN.md, "Seitenspalte"): 32 px rows;
 * the entry shown right now sits on the accent's soft fill with a bar at its start (not colour
 * alone). Every tree of the column uses these, so they read as one column. Section labels:
 * `SectionRow`.
 *
 * A row of a column tree; `extra` adds padding, indentation and drop states.
 */
export function sidebarRowClass(current: boolean, extra?: string | false): string {
  return cx(
    "relative flex h-8 min-w-0 items-center rounded-control text-13 transition-colors",
    current
      ? "bg-accent-soft font-medium text-primary before:absolute before:inset-y-2 before:start-0 before:w-1 before:rounded-control before:bg-accent-fill"
      : "text-secondary hover:bg-rail-hover hover:text-primary",
    extra,
  );
}

/** The icon of a row: accent while the row is the one shown. */
export function sidebarIconClass(current: boolean): string {
  return cx("size-(--ui-icon-small) shrink-0", current && "text-accent");
}

/**
 * Start padding per level (the token grid has no inline styles): 4 px, then 16 px a level, flat
 * from level 6. Padding instead of a margin, so the fill and the active bar span the whole row.
 */
const INDENT = [
  "ps-1",
  "ps-5",
  "ps-[calc(var(--ui-space-1)*9)]",
  "ps-[calc(var(--ui-space-1)*13)]",
  "ps-[calc(var(--ui-space-1)*17)]",
  "ps-[calc(var(--ui-space-1)*21)]",
] as const;

/** Indentation of a row at `level` (1 = none). */
export function indentClass(level: number): string {
  return INDENT[Math.min(Math.max(level - 1, 0), INDENT.length - 1)] ?? "";
}
