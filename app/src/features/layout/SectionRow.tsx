import { ChevronDown, ChevronRight } from "lucide-react";
import type { ReactNode } from "react";
import { cx } from "../../components/cx";
import { treeItemAria, type TreeNavItem, type useTreeNavigation } from "../../components/useTreeNavigation";
import { SECTION_HEAD_CLASS } from "./sidebarStyles";
import { useFold, type FoldKey } from "./sidebarFolds";

/** Key of a section's label row in its tree (entries use their own ids). */
export const SECTION_KEY = "section";

/**
 * A labelled section of the accounts-and-folders column as the root of its tree: open or folded
 * (stage 23, decision 0085: remembered on this device), its label row and the level of its
 * entries.
 */
export function useSection(key: FoldKey, label: string) {
  const fold = useFold(key);
  const item: TreeNavItem = { key: SECTION_KEY, level: 1, posinset: 1, setsize: 1, expanded: fold.open, text: label };
  return {
    open: fold.open,
    item,
    toggle: fold.toggle,
    /** For `useTreeNavigation`: Right and Left on the label row. */
    onExpand: (rowKey: string, expanded: boolean) => {
      if (rowKey === SECTION_KEY) fold.setOpen(expanded);
    },
  };
}

/**
 * The arrow of a section's label: shown on hover and focus, and always while the section is
 * folded, like the sidebars of macOS.
 */
export function SectionChevron({ open, end = true }: { open: boolean; end?: boolean }) {
  const Chevron = open ? ChevronDown : ChevronRight;
  return (
    <Chevron
      aria-hidden
      className={cx(
        "size-(--ui-icon-small) shrink-0",
        end && "ms-auto",
        open ? "opacity-0 group-hover/section:opacity-100 group-focus-visible/section:opacity-100" : "opacity-100",
      )}
      strokeWidth={1.75}
    />
  );
}

interface SectionRowProps {
  section: ReturnType<typeof useSection>;
  nav: ReturnType<typeof useTreeNavigation>;
  /**
   * Stage 17: the section's "…" menu (out of the Tab order), shown like the arrow; Shift+F10, the
   * context-menu key and a right click on the label open it too.
   */
  menu?: ReactNode;
  /** Shown at the end while the section is folded (the count of what it hides). */
  folded?: ReactNode;
  "data-testid"?: string;
}

/**
 * The quiet label of a section (11 px, semibold, tertiary) as the first row of its tree: a click,
 * Enter, Space, Left and Right fold and unfold it.
 */
export function SectionRow({ section, nav, menu, folded, ...rest }: SectionRowProps) {
  const { item, open, toggle } = section;
  return (
    <li
      {...treeItemAria(item)}
      {...nav.itemProps(item.key)}
      // The label alone, not the name of its "…" button too.
      aria-label={menu ? item.text : undefined}
      onClick={toggle}
      data-testid={rest["data-testid"]}
      className={SECTION_HEAD_CLASS}
    >
      <span className="truncate">{item.text}</span>
      {!open && folded}
      {menu && (
        // Its clicks stay here, so they do not also fold the section.
        <div
          onClick={(event) => {
            event.stopPropagation();
          }}
          className={cx(
            "flex opacity-0 group-hover/section:opacity-100 group-focus-visible/section:opacity-100 focus-within:opacity-100 has-[[aria-expanded=true]]:opacity-100",
            (open || !folded) && "ms-auto",
          )}
        >
          {menu}
        </div>
      )}
      <SectionChevron open={open} end={!menu && (open || !folded)} />
    </li>
  );
}
