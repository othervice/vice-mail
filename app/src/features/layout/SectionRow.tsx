import { ChevronDown, ChevronRight } from "lucide-react";
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
  "data-testid"?: string;
}

/**
 * The quiet label of a section (12 px, tertiary) as the first row of its tree: a click, Enter,
 * Space, Left and Right fold and unfold it. The arrow shows on hover and focus, and always while
 * the section is folded, like the sidebars of macOS.
 */
export function SectionRow({ section, nav, ...rest }: SectionRowProps) {
  const { item, open, toggle } = section;
  const Chevron = open ? ChevronDown : ChevronRight;
  return (
    <li
      {...treeItemAria(item)}
      {...nav.itemProps(item.key)}
      onClick={toggle}
      data-testid={rest["data-testid"]}
      className="group/section flex h-6 min-w-0 items-center gap-1 rounded-control px-2 text-12 font-semibold text-tertiary transition-colors hover:text-secondary"
    >
      <span className="truncate">{item.text}</span>
      <Chevron
        aria-hidden
        className={cx(
          "ms-auto size-(--ui-icon-small) shrink-0",
          open ? "opacity-0 group-hover/section:opacity-100 group-focus-visible/section:opacity-100" : "opacity-100",
        )}
        strokeWidth={1.75}
      />
    </li>
  );
}
