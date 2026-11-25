import { ChevronDown, ChevronRight } from "lucide-react";
import type { ReactNode } from "react";
import { cx } from "../../components/cx";
import { treeItemAria, type TreeNavItem, type useTreeNavigation } from "../../components/useTreeNavigation";
import { useUiStore } from "../../stores/ui";

/** Labelled sections of the column that fold to their label. */
export type SidebarSection = "views" | "savedSearches" | "favorites";

/** Key of a section's label row in its tree (entries use their own ids). */
export const SECTION_KEY = "section";

/**
 * A labelled section of the accounts-and-folders column as the root of its tree: open or folded
 * (remembered while the app runs), its label row and the level of its entries.
 */
export function useSection(section: SidebarSection, label: string) {
  const open = useUiStore((state) => !state.collapsedSections.has(section));
  const toggleSection = useUiStore((state) => state.toggleSection);
  const item: TreeNavItem = { key: SECTION_KEY, level: 1, posinset: 1, setsize: 1, expanded: open, text: label };
  return {
    open,
    item,
    toggle: () => {
      toggleSection(section);
    },
    /** For `useTreeNavigation`: Right and Left on the label row. */
    onExpand: (key: string, expanded: boolean) => {
      if (key === SECTION_KEY && open !== expanded) toggleSection(section);
    },
  };
}

interface SectionRowProps {
  section: ReturnType<typeof useSection>;
  nav: ReturnType<typeof useTreeNavigation>;
  /**
   * Stage 17: the section's "…" menu (out of the Tab order), shown like the arrow; Shift+F10, the
   * context-menu key and a right click on the label open it too.
   */
  menu?: ReactNode;
  "data-testid"?: string;
}

/**
 * The quiet label of a section (12 px, tertiary) as the first row of its tree: a click, Enter,
 * Space, Left and Right fold and unfold it. The arrow shows on hover and focus, and always while
 * the section is folded, like the sidebars of macOS.
 */
export function SectionRow({ section, nav, menu, ...rest }: SectionRowProps) {
  const { item, open, toggle } = section;
  const Chevron = open ? ChevronDown : ChevronRight;
  return (
    <li
      {...treeItemAria(item)}
      {...nav.itemProps(item.key)}
      // The label alone, not the name of its "…" button too.
      aria-label={menu ? item.text : undefined}
      onClick={toggle}
      data-testid={rest["data-testid"]}
      className="group/section flex h-6 min-w-0 items-center gap-1 rounded-control ps-3 pe-2 text-12 font-semibold text-tertiary transition-colors hover:text-secondary"
    >
      <span className="truncate">{item.text}</span>
      {menu && (
        // Its clicks stay here, so they do not also fold the section.
        <div
          onClick={(event) => {
            event.stopPropagation();
          }}
          className="ms-auto flex opacity-0 group-hover/section:opacity-100 group-focus-visible/section:opacity-100 focus-within:opacity-100 has-[[aria-expanded=true]]:opacity-100"
        >
          {menu}
        </div>
      )}
      <Chevron
        aria-hidden
        className={cx(
          "size-(--ui-icon-small) shrink-0",
          !menu && "ms-auto",
          open ? "opacity-0 group-hover/section:opacity-100 group-focus-visible/section:opacity-100" : "opacity-100",
        )}
        strokeWidth={1.75}
      />
    </li>
  );
}
