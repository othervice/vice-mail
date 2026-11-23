import { Search } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { treeItemAria, useTreeNavigation } from "../../components/useTreeNavigation";
import { useUiStore } from "../../stores/ui";
import { SECTION_KEY, SectionRow, useSection } from "../layout/SectionRow";
import { sidebarIconClass, sidebarRowClass } from "../layout/sidebarStyles";
import { useSavedSearches } from "./savedQueries";

/**
 * Stage 7: the saved searches as views of the accounts-and-folders column, in their order, right
 * below the views. Nothing while there are none ("Saved views" explains how to make one).
 */
export function SavedSearchNav() {
  const { t } = useTranslation();
  const view = useUiStore((state) => state.view);
  const showSavedSearch = useUiStore((state) => state.showSavedSearch);
  const saved = useSavedSearches().data ?? [];
  const headingId = useId();
  const section = useSection("savedSearches", t("savedSearches.view.sidebarLabel"));
  const openId = view.kind === "mail" && view.mailView === "savedViews" ? view.savedSearchId : undefined;
  const items = saved.map((search, index) => ({ key: String(search.id), level: 2, posinset: index + 1, setsize: saved.length, text: search.name }));
  const nav = useTreeNavigation({
    items: [section.item, ...(section.open ? items : [])],
    currentKey: openId === undefined ? null : String(openId),
    onOpen: (key) => {
      if (key === SECTION_KEY) section.toggle();
      const search = saved.find((entry) => String(entry.id) === key);
      if (search) showSavedSearch(search.id);
    },
    onExpand: section.onExpand,
  });
  if (saved.length === 0) return null;
  return (
    <section aria-label={t("savedSearches.view.sidebarLabel")} className="flex flex-col" data-testid="saved-search-nav">
      <h2 id={headingId} className="sr-only">
        {t("savedSearches.view.sidebarLabel")}
      </h2>
      <ul {...nav.treeProps} aria-labelledby={headingId} className="flex flex-col">
        <SectionRow section={section} nav={nav} data-testid="sidebar-section-savedSearches" />
        {(section.open ? saved : []).map((search, index) => {
          const item = items[index];
          if (!item) return null;
          const current = openId === search.id;
          return (
            <li
              key={search.id}
              {...treeItemAria(item)}
              {...nav.itemProps(item.key)}
              aria-current={current ? "page" : undefined}
              onClick={() => {
                showSavedSearch(search.id);
              }}
              className={sidebarRowClass(current, "gap-2 ps-3 pe-2")}
              data-testid="saved-search-nav-entry"
            >
              <Search aria-hidden className={sidebarIconClass(current)} strokeWidth={1.75} />
              <span className="truncate">{search.name}</span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
