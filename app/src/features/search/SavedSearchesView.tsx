import { ArrowLeft, Bookmark, SearchX } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { MAX_SAVED_SEARCH_NAME_CHARS, toCoreError, type Account, type SavedSearch, type SavedSearchId } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import type { ShortcutPlatform } from "../actions/shortcuts";
import { useSearchPages, type SearchInput } from "./queries";
import { useDeleteSavedSearch, useRenameSavedSearch, useSavedSearches } from "./savedQueries";
import { SearchResults } from "./SearchResults";
import { monthFirstFor } from "./terms";

/** Inline rename: Enter saves, Escape cancels; the error stays at the field. */
function RenameField({ search, onDone }: { search: SavedSearch; onDone: () => void }) {
  const { t } = useTranslation();
  const [name, setName] = useState(search.name);
  const rename = useRenameSavedSearch();
  const inputRef = useRef<HTMLInputElement>(null);
  const id = useId();
  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);
  const submit = () => {
    if (rename.isPending) return;
    rename.mutate({ id: search.id, name: name.trim() }, { onSuccess: onDone });
  };
  return (
    <div className="flex flex-col gap-2" data-testid="saved-search-rename">
      <div className="flex flex-wrap items-end gap-2">
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <label htmlFor={id} className="text-13 font-medium text-primary">
            {t("savedSearches.view.renameField", { name: search.name })}
          </label>
          <input
            ref={inputRef}
            id={id}
            type="text"
            value={name}
            maxLength={MAX_SAVED_SEARCH_NAME_CHARS}
            onChange={(event) => {
              setName(event.target.value);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                submit();
              } else if (event.key === "Escape") {
                event.preventDefault();
                event.stopPropagation();
                onDone();
              }
            }}
            autoComplete="off"
            spellCheck={false}
            className="h-8 min-w-0 rounded-control border border-border-strong bg-surface-raised px-2 text-14 text-primary"
          />
        </div>
        <Button onClick={submit} disabled={rename.isPending || !name.trim()}>
          {t("savedSearches.view.renameSave")}
        </Button>
        <Button variant="secondary" onClick={onDone}>
          {t("savedSearches.view.renameCancel")}
        </Button>
      </div>
      {rename.isError && <ErrorNotice context={t("savedSearches.view.renameFailed")} error={toCoreError(rename.error)} />}
    </div>
  );
}

/** Rename and delete of one saved search, with the confirmation before deleting. */
function useEntryActions(search: SavedSearch, onDeleted?: () => void) {
  const { t } = useTranslation();
  const [renaming, setRenaming] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const remove = useDeleteSavedSearch();
  const renameButton = useRef<HTMLDivElement>(null);
  const wasRenaming = useRef(false);
  useEffect(() => {
    // Back on the rename button after renaming or cancelling.
    if (wasRenaming.current && !renaming) renameButton.current?.querySelector("button")?.focus();
    wasRenaming.current = renaming;
  }, [renaming]);

  const buttons = (
    <div className="flex shrink-0 items-center gap-2">
      <div ref={renameButton}>
        <Button
          variant="secondary"
          aria-label={t("savedSearches.view.renameLabel", { name: search.name })}
          onClick={() => {
            setRenaming(true);
          }}
        >
          {t("savedSearches.view.rename")}
        </Button>
      </div>
      <Button
        variant="secondary"
        aria-label={t("savedSearches.view.deleteLabel", { name: search.name })}
        disabled={remove.isPending}
        onClick={() => {
          setConfirming(true);
        }}
      >
        {t("savedSearches.view.delete")}
      </Button>
    </div>
  );
  const extra = (
    <>
      {remove.isError && <ErrorNotice context={t("savedSearches.view.deleteFailed")} error={toCoreError(remove.error)} />}
      {confirming && (
        <ConfirmDialog
          title={t("savedSearches.view.deleteTitle", { name: search.name })}
          body={t("savedSearches.view.deleteBody")}
          confirmLabel={t("savedSearches.view.deleteConfirm")}
          cancelLabel={t("savedSearches.view.deleteCancel")}
          onConfirm={() => {
            setConfirming(false);
            remove.mutate(search.id, { onSuccess: () => onDeleted?.() });
          }}
          onCancel={() => {
            setConfirming(false);
          }}
          data-testid="saved-search-delete-dialog"
        />
      )}
    </>
  );
  const rename = renaming ? (
    <RenameField
      search={search}
      onDone={() => {
        setRenaming(false);
      }}
    />
  ) : null;
  return { buttons, extra, rename };
}

/** One saved search in the list of the view: its name opens its results. */
function SavedSearchRow({ search }: { search: SavedSearch }) {
  const { t } = useTranslation();
  const showSavedSearch = useUiStore((state) => state.showSavedSearch);
  const { buttons, extra, rename } = useEntryActions(search);
  return (
    <li className="flex flex-col gap-2 border-b border-border-subtle px-4 py-3" data-testid="saved-search-row">
      {rename ?? (
        <div className="flex items-center gap-3">
          <div className="flex min-w-0 flex-1 flex-col items-start gap-1">
            <button
              type="button"
              onClick={() => {
                showSavedSearch(search.id);
              }}
              className="max-w-full truncate rounded-control text-start text-14 font-medium text-accent hover:underline"
            >
              {search.name}
            </button>
            <p className="max-w-full truncate font-mono text-12 text-secondary">{t("savedSearches.view.query", { query: search.query })}</p>
          </div>
          {buttons}
        </div>
      )}
      {extra}
    </li>
  );
}

/** An open saved search: the way back, its name and query with rename and delete, then its hits. */
function SavedSearchResults({ search, accounts, platform }: { search: SavedSearch; accounts: readonly Account[]; platform: ShortcutPlatform }) {
  const { t } = useTranslation();
  const showMailView = useUiStore((state) => state.showMailView);
  const [monthFirst] = useState(() => monthFirstFor(navigator.language));
  const back = () => {
    showMailView("savedViews");
  };
  const { buttons, extra, rename } = useEntryActions(search, back);
  const input: SearchInput = { query: search.query, scope: { kind: "all" }, typing: false, monthFirst };
  const results = useSearchPages(input);
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="saved-search-open">
      <div className="flex shrink-0 flex-col gap-2 border-b border-border-subtle px-4 py-3">
        <div>
          <Button variant="quiet" onClick={back}>
            <ArrowLeft aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {t("savedSearches.view.back")}
          </Button>
        </div>
        {rename ?? (
          <div className="flex items-center gap-3">
            <h2 className="min-w-0 flex-1 truncate text-16 font-semibold text-primary">{search.name}</h2>
            {buttons}
          </div>
        )}
        <p className="truncate font-mono text-12 text-secondary">{t("savedSearches.view.query", { query: search.query })}</p>
        {extra}
      </div>
      <SearchResults query={results} input={input} accounts={accounts} platform={platform} />
    </div>
  );
}

interface SavedSearchesViewProps {
  /** The open saved search; null: the list of all of them. */
  savedSearchId: SavedSearchId | null;
  accounts: readonly Account[];
  platform: ShortcutPlatform;
}

/**
 * Stage 7: the "Saved views" view of the rail. The saved searches (synced between devices) with
 * rename and delete, or the results of one of them over all accounts. Nothing invented: without
 * saved searches it says how to make one.
 */
export function SavedSearchesView({ savedSearchId, accounts, platform }: SavedSearchesViewProps) {
  const { t } = useTranslation();
  const saved = useSavedSearches();
  const showMailView = useUiStore((state) => state.showMailView);
  if (saved.isPending) return null;
  if (saved.isError) {
    return (
      <div className="px-6 py-4">
        <ErrorNotice
          context={t("savedSearches.view.loadFailed")}
          error={toCoreError(saved.error)}
          onRetry={() => {
            void saved.refetch();
          }}
        />
      </div>
    );
  }
  if (savedSearchId !== null) {
    const search = saved.data.find((s) => s.id === savedSearchId);
    if (!search) {
      return (
        <EmptyState
          icon={SearchX}
          title={t("savedSearches.view.gone.title")}
          body={t("savedSearches.view.gone.body")}
          action={{
            label: t("savedSearches.view.back"),
            onClick: () => {
              showMailView("savedViews");
            },
          }}
        />
      );
    }
    return <SavedSearchResults key={search.id} search={search} accounts={accounts} platform={platform} />;
  }
  if (saved.data.length === 0) {
    return <EmptyState icon={Bookmark} title={t("savedSearches.view.empty.title")} body={t("savedSearches.view.empty.body")} />;
  }
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <ul aria-label={t("savedSearches.view.listLabel")} className="flex flex-col" data-testid="saved-search-list">
        {saved.data.map((search) => (
          <SavedSearchRow key={search.id} search={search} />
        ))}
      </ul>
    </div>
  );
}
