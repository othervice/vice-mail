import { useRef, useState, type ReactNode } from "react";
import { toCoreError, type AccountGroupId, type AccountId, type MailboxId, type SearchScope } from "../../services/ipc";
import { useSearchStore, type SearchScopeChoice } from "../../stores/search";
import type { ShortcutPlatform } from "../actions/shortcuts";
import { useAccounts } from "../accounts/queries";
import { useSearchPages, type SearchInput } from "./queries";
import { useAddRecentSearch } from "./savedQueries";
import { SearchBar } from "./SearchBar";
import { SearchResults } from "./SearchResults";
import { monthFirstFor } from "./terms";
import { useDebounced } from "./useDebounced";

/** Wait this long after the last keystroke before searching while typing. */
export const TYPING_DELAY_MS = 150;

/** What the mail view shows, which decides the scopes a search can have. */
export interface SearchContext {
  accountId: AccountId | null;
  mailboxId: MailboxId | null;
  groupId: AccountGroupId | null;
}

/** The scopes that exist in a view, narrowest first; "all" always. */
export function scopesFor(context: SearchContext): SearchScopeChoice[] {
  return [
    ...(context.mailboxId !== null ? (["mailbox"] as const) : []),
    ...(context.accountId !== null ? (["account"] as const) : []),
    ...(context.groupId !== null ? (["group"] as const) : []),
    "all",
  ];
}

/** The core's scope for a choice; a choice the view does not have falls back to all accounts. */
export function scopeOf(choice: SearchScopeChoice, context: SearchContext): SearchScope {
  if (choice === "mailbox" && context.mailboxId !== null) return { kind: "mailbox", mailboxId: context.mailboxId };
  if (choice === "account" && context.accountId !== null) return { kind: "account", accountId: context.accountId };
  if (choice === "group" && context.groupId !== null) return { kind: "group", groupId: context.groupId };
  return { kind: "all" };
}

interface SearchAreaProps {
  context: SearchContext;
  platform: ShortcutPlatform;
  /** The view's own content: shown while the search is closed or its field is empty. */
  children: ReactNode;
}

/** Puts the focus back on the list of the view after the search closed. */
function focusList() {
  window.requestAnimationFrame(() => {
    document.querySelector<HTMLElement>("[data-testid='message-list']")?.focus({ preventScroll: true });
  });
}

/**
 * The search of a mail view: the field under the header while it is open, and the results in
 * place of the view's list as soon as there is something to search for.
 */
export function SearchArea({ context, platform, children }: SearchAreaProps) {
  const open = useSearchStore((state) => state.open);
  const query = useSearchStore((state) => state.query);
  const submitted = useSearchStore((state) => state.submitted);
  const choice = useSearchStore((state) => state.scope);
  const closeSearch = useSearchStore((state) => state.closeSearch);
  const accounts = useAccounts().data ?? [];
  // The region's date order, for `01/02/2026` (decision 0013); never changes while running.
  const [monthFirst] = useState(() => monthFirstFor(navigator.language));
  const scopes = scopesFor(context);
  const effective = scopes.includes(choice) ? choice : "all";
  const text = useDebounced(query, submitted ? 0 : TYPING_DELAY_MS);
  const searched = submitted ? query : text;
  const input: SearchInput | null =
    open && searched.trim() ? { query: searched, scope: scopeOf(effective, context), typing: !submitted, monthFirst } : null;
  const results = useSearchPages(input);
  const page = results.isPlaceholderData ? undefined : results.data?.pages[0];
  // Stage 7: a search counts as used (a recent search) on Enter, a picked recent search, or
  // when one of its hits is opened. The core moves a repeated one to the top.
  const addRecent = useAddRecentSearch();
  const recorded = useRef<string | null>(null);
  const record = (text: string) => {
    const trimmed = text.trim();
    if (!trimmed) return;
    recorded.current = trimmed;
    addRecent.mutate(trimmed);
  };
  const recordUse = () => {
    if (input && input.query.trim() !== recorded.current) record(input.query);
  };

  return (
    <>
      {open && (
        <SearchBar
          scopes={scopes}
          scope={effective}
          page={page}
          searched={input ? input.query : null}
          platform={platform}
          onClose={() => {
            closeSearch();
            focusList();
          }}
          onRecord={record}
          recordError={addRecent.isError ? toCoreError(addRecent.error) : null}
        />
      )}
      {open && input ? <SearchResults query={results} input={input} accounts={accounts} platform={platform} onUse={recordUse} /> : children}
    </>
  );
}
