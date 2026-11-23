import { Bookmark } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
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
  if (saved.length === 0) return null;
  const openId = view.kind === "mail" && view.mailView === "savedViews" ? view.savedSearchId : undefined;
  return (
    <section aria-label={t("savedSearches.view.sidebarLabel")} className="flex flex-col gap-1" data-testid="saved-search-nav">
      <h2 className="flex h-8 items-center px-2 text-12 font-semibold text-secondary">{t("savedSearches.view.sidebarLabel")}</h2>
      <ul className="flex flex-col">
        {saved.map((search) => {
          const current = openId === search.id;
          return (
            <li key={search.id}>
              <button
                type="button"
                aria-current={current ? "page" : undefined}
                onClick={() => {
                  showSavedSearch(search.id);
                }}
                className={cx(
                  "flex h-8 w-full min-w-0 items-center gap-2 rounded-control px-2 text-13 transition-colors",
                  current ? "bg-selected font-medium text-primary" : "text-secondary hover:bg-hover hover:text-primary",
                )}
                data-testid="saved-search-nav-entry"
              >
                <Bookmark aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
                <span className="truncate">{search.name}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
