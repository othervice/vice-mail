import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { MAX_SAVED_SEARCH_NAME_CHARS, toCoreError, type SavedSearch } from "../../services/ipc";
import { useSaveSearch } from "./savedQueries";

interface SaveSearchFormProps {
  /** The search as typed; saved as it is. */
  query: string;
  onSaved: (saved: SavedSearch) => void;
  onCancel: () => void;
}

/**
 * "Save search" under the search field (stage 7): a name, prefilled with the query, and the note
 * that a saved search looks in all accounts. Not a `<form>`: it sits inside the search form, so
 * Enter and Escape are handled here and go no further.
 */
export function SaveSearchForm({ query, onSaved, onCancel }: SaveSearchFormProps) {
  const { t } = useTranslation();
  const [name, setName] = useState(() => query.trim().slice(0, MAX_SAVED_SEARCH_NAME_CHARS));
  const save = useSaveSearch();
  const inputRef = useRef<HTMLInputElement>(null);
  const nameId = useId();
  const noteId = useId();

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const submit = () => {
    if (save.isPending) return;
    save.mutate(
      { name: name.trim(), query: query.trim() },
      {
        onSuccess: (saved) => {
          onSaved(saved);
        },
      },
    );
  };

  return (
    <div role="group" aria-label={t("savedSearches.save.formLabel")} className="flex flex-col gap-2" data-testid="search-save-form">
      <div className="flex flex-wrap items-end gap-2">
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <label htmlFor={nameId} className="text-13 font-medium text-primary">
            {t("savedSearches.save.name")}
          </label>
          <input
            ref={inputRef}
            id={nameId}
            type="text"
            value={name}
            maxLength={MAX_SAVED_SEARCH_NAME_CHARS}
            onChange={(event) => {
              setName(event.target.value);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                event.stopPropagation();
                submit();
              } else if (event.key === "Escape") {
                event.preventDefault();
                event.stopPropagation();
                onCancel();
              }
            }}
            aria-describedby={noteId}
            autoComplete="off"
            spellCheck={false}
            className="h-8 min-w-0 rounded-control border border-border-strong bg-surface-raised px-2 text-14 text-primary"
            data-testid="search-save-name"
          />
        </div>
        <Button onClick={submit} disabled={save.isPending || !name.trim()} data-testid="search-save-submit">
          {t("savedSearches.save.submit")}
        </Button>
        <Button variant="secondary" onClick={onCancel}>
          {t("savedSearches.save.cancel")}
        </Button>
      </div>
      <p id={noteId} className="text-12 text-secondary">
        {t("savedSearches.save.scopeNote")}
      </p>
      {save.isError && <ErrorNotice context={t("savedSearches.save.failed")} error={toCoreError(save.error)} data-testid="search-save-error" />}
    </div>
  );
}
