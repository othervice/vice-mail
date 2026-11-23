import { Bookmark } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { treeItemAria, useTreeNavigation } from "../../components/useTreeNavigation";
import { useUiStore } from "../../stores/ui";
import { useSavedSearches } from "./savedQueries";

/**
 * Stage 7: the saved searches as views of the unfolded sidebar, in their order. Nothing while
 * there are none (the "Saved views" entry of the rail explains how to make one).
 */
export function SavedSearchNav() {
  const { t } = useTranslation();
  const view = useUiStore((state) => state.view);
  const showSavedSearch = useUiStore((state) => state.showSavedSearch);
  const saved = useSavedSearches().data ?? [];
  const headingId = useId();
  const openId = view.kind === "mail" && view.mailView === "savedViews" ? view.savedSearchId : undefined;
  const items = saved.map((search, index) => ({ key: String(search.id), level: 1, posinset: index + 1, setsize: saved.length, text: search.name }));
  const nav = useTreeNavigation({
    items,
    currentKey: openId === undefined ? null : String(openId),
    onOpen: (key) => {
      const search = saved.find((entry) => String(entry.id) === key);
      if (search) showSavedSearch(search.id);
    },
  });
  if (saved.length === 0) return null;
  return (
    <section aria-label={t("savedSearches.view.sidebarLabel")} className="flex flex-col gap-1" data-testid="saved-search-nav">
      <h2 id={headingId} className="flex h-8 items-center px-2 text-12 font-semibold text-secondary">
        {t("savedSearches.view.sidebarLabel")}
      </h2>
      <ul {...nav.treeProps} aria-labelledby={headingId} className="flex flex-col">
        {saved.map((search, index) => {
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
              className={cx(
                "flex h-8 w-full min-w-0 items-center gap-2 rounded-control px-2 text-13 transition-colors",
                current ? "bg-selected font-medium text-primary" : "text-secondary hover:bg-rail-hover hover:text-primary",
              )}
              data-testid="saved-search-nav-entry"
            >
              <Bookmark aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
              <span className="truncate">{search.name}</span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
