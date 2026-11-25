import { History } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import type { RecentSearch } from "../../services/ipc";

interface RecentSearchesProps {
  /** Id of the listbox; the search field names it in `aria-controls`. */
  id: string;
  items: readonly RecentSearch[];
  /** The option the arrow keys moved to (-1: none); the field points at it with `aria-activedescendant`. */
  active: number;
  optionId: (index: number) => string;
  onPick: (query: string) => void;
  onClear: () => void;
  clearing: boolean;
}

/**
 * The recent searches of this device under the empty search field (stage 7). The focus stays in
 * the field: the arrow keys move through the list, Enter runs the search, Escape closes the list.
 * A click runs it too (the mouse down does not take the focus from the field).
 */
export function RecentSearches({ id, items, active, optionId, onPick, onClear, clearing }: RecentSearchesProps) {
  const { t } = useTranslation();
  return (
    <div
      className="absolute inset-x-0 top-full z-20 mt-1 flex flex-col overflow-hidden rounded-card border border-border-strong bg-surface-raised shadow-floating"
      data-testid="search-recent"
    >
      <ul id={id} role="listbox" aria-label={t("savedSearches.recent.label")} className="flex max-h-(--ui-picker-list-height) flex-col overflow-y-auto py-1">
        {items.map((item, index) => (
          <li
            key={item.query}
            id={optionId(index)}
            role="option"
            aria-selected={index === active}
            onMouseDown={(event) => {
              event.preventDefault();
            }}
            onClick={() => {
              onPick(item.query);
            }}
            className={cx(
              "flex h-8 cursor-default items-center gap-2 px-3 text-13 text-primary",
              index === active ? "bg-selected" : "hover:bg-hover",
            )}
            data-testid="search-recent-option"
          >
            <History aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
            <span className="truncate font-mono">{item.query}</span>
          </li>
        ))}
      </ul>
      <div className="border-t border-border-subtle px-2 py-1">
        <button
          type="button"
          disabled={clearing}
          onMouseDown={(event) => {
            event.preventDefault();
          }}
          onClick={onClear}
          className="h-8 rounded-control px-2 text-12 text-accent hover:bg-hover disabled:opacity-50"
          data-testid="search-recent-clear"
        >
          {t("savedSearches.recent.clear")}
        </button>
      </div>
    </div>
  );
}
