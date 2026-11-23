import { ArrowLeft, SearchX, Server } from "lucide-react";
import { useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Notice } from "../../components/Notice";
import type { Account, MessageId, MessageListItem, SearchHit } from "../../services/ipc";
import type { ShortcutPlatform } from "../actions/shortcuts";
import type { InfiniteData, UseInfiniteQueryResult } from "@tanstack/react-query";
import type { ListPage, MessageListSource } from "../messages/listSource";
import { MessageList, NO_SELECTION, type ListSelection, type MessageListHandle } from "../messages/MessageList";
import { MIXED_LIST_SCOPE } from "../../stores/listSelect";
import { buildRows } from "../messages/rows";
import type { FocusRequest } from "../reading/ReadingPane";
import { useReadingLayout } from "../layout/useReadingLayout";
import { MixedReading } from "../unified/MixedReading";
import { useServerSearchPages, type SearchInput } from "./queries";
import { accountShortNames } from "../accounts/accountName";

/** What the result lists need of a search query: pages with hits and their rows. */
type HitPage = ListPage & { hits: readonly SearchHit[]; matchesNothing?: boolean };
type HitQuery = UseInfiniteQueryResult<InfiniteData<HitPage, unknown>>;

/** Rows of hits from any account: the account colour and the found places of each. */
function useHitSource(query: HitQuery, accounts: readonly Account[]): { source: MessageListSource; hitById: ReadonlyMap<MessageId, SearchHit> } {
  const pages = query.data?.pages;
  const hitById = useMemo(
    () => new Map((pages ?? []).flatMap((page) => page.hits).map((hit) => [hit.message.id, hit] as const)),
    [pages],
  );
  const colors = useMemo(() => new Map(accounts.map((account) => [account.id, account.color] as const)), [accounts]);
  // Stage 8: with more than one account, each hit names its account next to the colour.
  const names = useMemo(() => (accounts.length > 1 ? accountShortNames(accounts) : null), [accounts]);
  const source: MessageListSource = {
    query,
    buildRows,
    accountColor: (item) => colors.get(item.accountId) ?? 0,
    ...(names && { accountName: (item: MessageListItem) => names.get(item.accountId) }),
    decorate: (item) => {
      const hit = hitById.get(item.id);
      return hit ? { highlights: hit.highlights } : undefined;
    },
  };
  return { source, hitById };
}

interface HitListProps {
  query: HitQuery;
  accounts: readonly Account[];
  label: string;
  platform: ShortcutPlatform;
  /** Above the list (notices of the server search, the way back). */
  header?: ReactNode;
  /** Below the list (the button for the server search). */
  footer?: ReactNode;
  /** Stage 7: a hit was selected or opened (the search counts as a recent one). */
  onUse?: (() => void) | undefined;
}

/**
 * Hits in the message list (same rows, keys and virtualization as a folder) and the selected one
 * in the reading pane, with the actions of the hit's own account and folder. Quick actions in the
 * rows and the list shortcuts stay with the folder view: a page of hits mixes folders and
 * accounts, and the action controller works per folder.
 */
function HitList({ query, accounts, label, platform, header, footer, onUse }: HitListProps) {
  const [selection, setSelection] = useState<ListSelection>(NO_SELECTION);
  const [focusRequest, setFocusRequest] = useState<FocusRequest | null>(null);
  const list = useRef<MessageListHandle>(null);
  const { t } = useTranslation();
  const { source, hitById } = useHitSource(query, accounts);
  const single = selection.items.length === 1 ? (selection.items[0] ?? null) : null;
  const account = single ? accounts.find((a) => a.id === single.accountId) : undefined;
  const open = Boolean(single && account);
  // Stage 8: the reading pane right, below or in place of the list.
  const layout = useReadingLayout(list);

  return (
    <div className={layout.container} data-reading-pane={layout.position}>
      <div {...layout.listColumn(open)}>
        {header}
        <MessageList
          source={source}
          folderName={label}
          showRecipients={false}
          platform={platform}
          handle={list}
          // Stage 9 (decision 0028): an AI source among these rows is selected here.
          selectScope={MIXED_LIST_SCOPE}
          onSelectedById={() => {
            layout.onOpen();
          }}
          onSelectionChange={(next) => {
            setSelection(next);
            const focusPane = layout.onSelectionChange(next);
            const only = next.items.length === 1 ? next.items[0]?.id : undefined;
            if (only !== undefined) onUse?.();
            setFocusRequest((request) =>
              request && request.messageId === only
                ? request
                : focusPane && only !== undefined
                  ? { messageId: only, seq: (request?.seq ?? 0) + 1 }
                  : null,
            );
          }}
          onOpen={(id: MessageId) => {
            onUse?.();
            layout.onOpen();
            setFocusRequest((request) => ({ messageId: id, seq: (request?.seq ?? 0) + 1 }));
          }}
        />
        {footer}
      </div>
      {single && account && layout.paneShown(open) && (
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          {hitById.get(single.id)?.bodyLocal === false && (
            <p className="shrink-0 border-b border-border-subtle px-4 py-2 text-12 text-secondary" data-testid="search-headers-only">
              {t("search.results.headersOnly")}
            </p>
          )}
          <MixedReading
          key={single.id}
          account={account}
          item={single}
          focusRequest={focusRequest}
          platform={platform}
          list={list}
          layout={layout}
          />
        </div>
      )}
    </div>
  );
}

function ServerButton({ onClick }: { onClick: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 border-t border-border-subtle px-4 py-2">
      <Button variant="secondary" onClick={onClick} data-testid="search-server">
        <Server aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
        {t("search.server.action")}
      </Button>
      <p className="text-12 text-secondary">{t("search.server.description")}</p>
    </div>
  );
}

/** "Search on the server": its hits, what could not be searched, and the way back. */
function ServerResults({ input, accounts, platform, onBack }: { input: SearchInput; accounts: readonly Account[]; platform: ShortcutPlatform; onBack: () => void }) {
  const { t } = useTranslation();
  const query = useServerSearchPages(input);
  const result = query.data?.pages[0];
  const back = (
    <div className="flex shrink-0 flex-col gap-2 px-4 pt-3 pb-2">
      <div>
        <Button variant="quiet" onClick={onBack} data-testid="search-server-back">
          <ArrowLeft aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t("search.server.back")}
        </Button>
      </div>
      <h2 className="text-14 font-semibold text-primary">{t("search.server.title")}</h2>
      {query.isFetching && (
        <p role="status" className="text-13 text-secondary">
          {t("search.server.running")}
        </p>
      )}
      {result && (
        <div role="status" className="flex flex-col gap-1 text-12 text-secondary" data-testid="search-server-status">
          {result.failed.length > 0 && <p className="text-warning">{t("search.server.failedFolders", { count: result.failed.length })}</p>}
          {result.notSynced > 0 && <p>{t("search.server.notSynced", { count: result.notSynced })}</p>}
          {result.truncated && <p>{t("search.server.truncated")}</p>}
        </div>
      )}
    </div>
  );

  if (query.isError) {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        {back}
        <div className="px-4">
          <ErrorNotice
            context={t("search.server.failed")}
            error={query.error}
            onRetry={() => {
              void query.refetch();
            }}
          />
        </div>
      </div>
    );
  }
  if (result && result.hits.length === 0) {
    // Without a word nothing is sent to the server, so no folder was searched.
    const filtersOnly = result.searched.length === 0 && result.failed.length === 0;
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        {back}
        <div className="px-4">
          <Notice title={filtersOnly ? t("search.server.filtersOnly") : t("search.server.none")} data-testid="search-server-none" />
        </div>
      </div>
    );
  }
  return <HitList query={query} accounts={accounts} label={t("search.server.listName")} platform={platform} header={back} />;
}

interface SearchResultsProps {
  query: HitQuery & { isPlaceholderData: boolean };
  input: SearchInput;
  accounts: readonly Account[];
  platform: ShortcutPlatform;
  /** Stage 7: a hit was selected or opened. */
  onUse?: () => void;
}

/**
 * Results of a search in place of the folder's list: hits as rows with the found places marked,
 * clear empty states (nothing found, or the query cannot find anything), errors at the place,
 * and the way to search on the server for mail whose content is not stored here.
 */
export function SearchResults({ query, input, accounts, platform, onUse }: SearchResultsProps) {
  const { t } = useTranslation();
  const [server, setServer] = useState<SearchInput | null>(null);
  const first = query.data?.pages[0];
  const label = t("search.results.label", { query: input.query.trim() });
  const searchServer = () => {
    setServer({ ...input, typing: false });
  };

  if (server && server.query === input.query) {
    return <ServerResults input={server} accounts={accounts} platform={platform} onBack={() => { setServer(null); }} />;
  }
  if (query.isError && !query.data) {
    return (
      <div className="px-6 py-4">
        <ErrorNotice
          context={t("search.results.failed")}
          error={query.error}
          onRetry={() => {
            void query.refetch();
          }}
        />
      </div>
    );
  }
  if (first && !query.isPlaceholderData && query.data?.pages.every((page) => page.hits.length === 0)) {
    if (first.matchesNothing) {
      return <EmptyState icon={SearchX} title={t("search.results.matchesNothing.title")} body={t("search.results.matchesNothing.body")} />;
    }
    return (
      <div className="flex min-h-0 flex-1 flex-col" data-testid="search-empty">
        <EmptyState icon={SearchX} title={t("search.results.empty.title")} body={t("search.results.empty.body", { query: input.query.trim() })} />
        <ServerButton onClick={searchServer} />
      </div>
    );
  }
  return (
    <HitList
      query={query}
      accounts={accounts}
      label={label}
      platform={platform}
      onUse={onUse}
      // Once the last page is there: the server is for what the local results do not have.
      footer={query.isSuccess && !query.hasNextPage && !query.isPlaceholderData ? <ServerButton onClick={searchServer} /> : null}
    />
  );
}
