import { EyeOff, FolderX, Inbox } from "lucide-react";
import { useMemo, useRef, useState, type ReactNode } from "react";
import type { QueryKey } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { Account, AccountColor, InboxScope, MessageId, MessageListItem, UnifiedView } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import type { ShortcutPlatform } from "../actions/shortcuts";
import type { MessageListSource } from "../messages/listSource";
import { MessageList, NO_SELECTION, type ListSelection, type MessageListHandle } from "../messages/MessageList";
import type { ListTableConfig } from "../views/TableHeader";
import { MIXED_LIST_SCOPE } from "../../stores/listSelect";
import { useUnifiedInboxSource } from "../comfort/listSources";
import { ListResizeHandle } from "../layout/ListResizeHandle";
import { NoMessageSelected } from "../reading/NoMessageSelected";
import { usePreviewing } from "../reading/usePreviewing";
import { useReadingLayout } from "../layout/useReadingLayout";
import { MixedReading } from "./MixedReading";
import type { FocusRequest } from "../reading/ReadingPane";
import { useAccountGroups, useHiddenAccounts } from "./queries";
import { accountShortNames } from "../accounts/accountName";
import { BlockedHiddenNote } from "../blocked/BlockedHiddenNote";
import { unreadViewKey, useUnreadMode, useUnreadSource } from "../organize/unread";
import { useConversationsOn } from "../threads/preference";
import { useListView } from "../views/useListView";
import { useMixedActions } from "./MixedActions";
import { unionIds, useDropRows } from "../organize/useDropRows";

/** The accounts a unified view mixes: visible ones, of the group if it is a group view. */
export function visibleAccounts(
  accounts: readonly Account[],
  hidden: readonly string[],
  memberIds: readonly string[] | null,
): Account[] {
  return accounts.filter((account) => !hidden.includes(account.id) && (memberIds === null || memberIds.includes(account.id)));
}

/** What actions in the unified inbox reload: every list under `["unified"]`. */
const UNIFIED_LIST_KEY = ["unified"] as const;

interface PanesProps {
  view: UnifiedView;
  accounts: readonly Account[];
  name: string;
  platform: ShortcutPlatform;
}

/**
 * The list of a unified view and the reading pane: the sorted inbox (stage 9: blocks, bundles,
 * hidden snoozes) over every shown account, each row with its account's stripe (stage 23),
 * opened with the account it belongs to. Decision 0039: with the conversation view on (the
 * default) each row is a conversation of its account and opens in the thread pane.
 */
function UnifiedPanes({ view, accounts, name, platform }: PanesProps) {
  const { t } = useTranslation();
  const [selection, setSelection] = useState<ListSelection>(NO_SELECTION);
  // Stage 16: "Unread only" and "Unread first" over the same accounts.
  const unreadScope: InboxScope = { kind: "unified", view };
  const unreadMode = useUnreadMode(unreadViewKey(unreadScope));
  const conversations = useConversationsOn();
  // Stage 19 (decisions 0061, 0062): the view's order, its quick filter and the table.
  const listView = useListView({
    scope: unreadScope,
    keyBase: ["unified", view],
    conversations,
    kept: selection.items,
    showRecipients: false,
    mixedAccounts: accounts.length > 1,
  });
  const unread = useUnreadSource(unreadScope, ["unified", view], listView.active ? null : unreadMode, selection.items);
  const sorted = useUnifiedInboxSource(view, !unread && !listView.active);
  const source: MessageListSource = {
    empty: { icon: Inbox, title: t("unified.empty.title"), body: t("unified.empty.body") },
    ...(listView.source ?? unread ?? sorted),
  };
  return (
    <MixedPanes
      accounts={accounts}
      source={source}
      listKey={UNIFIED_LIST_KEY}
      name={name}
      platform={platform}
      conversations={conversations}
      selection={selection}
      onSelection={setSelection}
      errorContext={t("unified.loadFailed")}
      table={listView.table}
      top={
        <>
          {/* Stage 16 (decision 0036): mail of blocked senders the unified inbox leaves out. */}
          <BlockedHiddenNote scope={unreadScope} accountId={null} />
          {listView.bar}
        </>
      }
      data-testid="unified-inbox"
    />
  );
}

interface MixedPanesProps {
  /** Every account (colours, names and actions of the rows). */
  accounts: readonly Account[];
  /** The rows; the account's colour and name are added here. */
  source: MessageListSource;
  /** The mixed list's query, reloaded after actions that move rows out of it. */
  listKey: QueryKey;
  name: string;
  platform: ShortcutPlatform;
  /** Decision 0039: the rows are conversations. */
  conversations: boolean;
  /** Sent: the rows name their recipients instead of the sender. */
  showRecipients?: boolean;
  selection: ListSelection;
  onSelection: (selection: ListSelection) => void;
  /** What the error notice names when the list cannot be loaded. */
  errorContext: string;
  /** Above the list (notes, the quick filter's bar). */
  top?: ReactNode;
  table?: ListTableConfig | undefined;
  "data-testid": string;
}

/**
 * A list that mixes accounts and its reading pane (stage 7; stage 23, decision 0087: also Later,
 * Flagged, Sent and the other views of "All accounts"): each row with its account's stripe, its
 * quick actions, context menu and drags acting with its own account and folder (decision 0065),
 * opened with that account (its read-only rules, its actions).
 */
export function MixedPanes({
  accounts,
  source: rows,
  listKey,
  name,
  platform,
  conversations,
  showRecipients = false,
  selection,
  onSelection,
  errorContext,
  top,
  table,
  ...rest
}: MixedPanesProps) {
  const [focusRequest, setFocusRequest] = useState<FocusRequest | null>(null);
  const list = useRef<MessageListHandle>(null);
  const colors = useMemo(() => new Map(accounts.map((account) => [account.id, account.color] as const)), [accounts]);
  const accountColor = (item: MessageListItem): AccountColor => colors.get(item.accountId) ?? 0;
  // Stage 8: with more than one account, each row names its account (stage 23: the stripe's name).
  const names = useMemo(() => (accounts.length > 1 ? accountShortNames(accounts) : null), [accounts]);
  const source: MessageListSource = {
    ...rows,
    accountColor,
    ...(names && { accountName: (item: MessageListItem) => names.get(item.accountId) }),
  };
  const query = source.query;
  // Stage 21 (decision 0065): quick actions and the context menu with each row's own account.
  const loaded = useMemo(() => query.data?.pages.flatMap((page) => page.items) ?? [], [query.data]);
  const mixed = useMixedActions({ accounts, list, conversations, items: loaded, listKey, platform });
  // Stage 22 (decision 0081): rows of several accounts dragged onto any folder.
  const drops = useDropRows({ list, conversations, listKey });
  const hiddenIds = useMemo(() => unionIds(mixed.hiddenIds, drops.hiddenIds), [mixed.hiddenIds, drops.hiddenIds]);
  const single = selection.items.length === 1 ? (selection.items[0] ?? null) : null;
  const rowKey = selection.keys[0] ?? "";
  const account = single ? accounts.find((a) => a.id === single.accountId) : undefined;
  // Stage 8: the reading pane right, below or in place of the list.
  const layout = useReadingLayout(list);
  const open = Boolean(single && account);
  // Stage 18 (decision 0054): the top message the list opened by itself is not read yet.
  const preview = usePreviewing(selection);

  if (query.isError && !query.data) {
    return (
      <div className="px-6 py-4">
        <ErrorNotice
          context={errorContext}
          error={query.error}
          onRetry={() => {
            void query.refetch();
          }}
        />
      </div>
    );
  }

  return (
    <div className={layout.container} data-testid={rest["data-testid"]} data-reading-pane={layout.position}>
      <div {...layout.listColumn(open)}>
        {mixed.element}
        {top}
        {source.controls}
        <MessageList
          source={source}
          folderName={name}
          showRecipients={showRecipients}
          platform={platform}
          handle={list}
          // Stage 9 (decision 0028): an AI source among these rows is selected here.
          selectScope={MIXED_LIST_SCOPE}
          onSelectedById={() => {
            layout.onOpen();
          }}
          onSelectionChange={(next) => {
            onSelection(next);
            const focusPane = layout.onSelectionChange(next);
            const only = next.items.length === 1 ? next.items[0]?.id : undefined;
            setFocusRequest((request) =>
              request && request.messageId === only
                ? request
                : focusPane && only !== undefined
                  ? { messageId: only, seq: (request?.seq ?? 0) + 1 }
                  : null,
            );
          }}
          onOpen={(id: MessageId) => {
            layout.onOpen();
            preview.engage();
            setFocusRequest((request) => ({ messageId: id, seq: (request?.seq ?? 0) + 1 }));
          }}
          // Stage 18 (task F): opening the list shows its top message.
          autoSelectFirst={layout.autoSelectFirst}
          table={table}
          hiddenIds={hiddenIds}
          renderQuickActions={mixed.renderQuickActions}
          onRowMenu={mixed.onRowMenu}
          onDragRows={drops.onDragRows}
          onDragEnd={drops.onDragEnd}
        />
      </div>
      {/* Stage 18: the border between list and pane can be dragged. */}
      <ListResizeHandle layout={layout} />
      {layout.placeholderShown(open) && <NoMessageSelected />}
      {single && account && layout.paneShown(open) && (
        <div className="contents" {...preview.paneProps}>
          <MixedReading
            markRead={!preview.previewing}
            // Stage 17: a conversation row keeps its pane while a newer message becomes the row's.
            key={rowKey || single.id}
            account={account}
            item={single}
            focusRequest={focusRequest}
            platform={platform}
            list={list}
            layout={layout}
            conversation={conversations && !selection.childIds.has(single.id)}
            ident={rowKey}
            onShowMessage={(messageId) => {
              list.current?.showInConversation(rowKey, messageId);
            }}
          />
        </div>
      )}
    </div>
  );
}

/** The name a unified view has in headers and lists: "All inboxes" or the group's name. */
export function useUnifiedName(view: UnifiedView): string {
  const { t } = useTranslation();
  const groups = useAccountGroups(view.kind === "group");
  if (view.kind === "all") return t("unified.title");
  return groups.data?.find((group) => group.id === view.groupId)?.name ?? t("unified.title");
}

interface UnifiedInboxProps {
  view: UnifiedView;
  accounts: readonly Account[];
  platform: ShortcutPlatform;
}

/**
 * The unified inbox (stage 7): the inboxes of every visible account, or of a group's visible
 * accounts, mixed by time. Hidden accounts are left out by the core; the view says so when
 * nothing is left to show.
 */
export function UnifiedInbox({ view, accounts, platform }: UnifiedInboxProps) {
  const { t } = useTranslation();
  const groups = useAccountGroups();
  const hidden = useHiddenAccounts();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const name = useUnifiedName(view);

  if (groups.isPending || hidden.isPending) return null;
  if (groups.isError || hidden.isError) {
    const error = groups.error ?? hidden.error;
    return (
      <div className="px-6 py-4">
        {error && (
          <ErrorNotice
            context={t("unified.groupsFailed")}
            error={error}
            onRetry={() => {
              void groups.refetch();
              void hidden.refetch();
            }}
          />
        )}
      </div>
    );
  }
  const group = view.kind === "group" ? groups.data.find((g) => g.id === view.groupId) : undefined;
  if (view.kind === "group" && !group) {
    return <EmptyState icon={FolderX} title={t("unified.groupGone.title")} body={t("unified.groupGone.body")} />;
  }
  const shown = visibleAccounts(accounts, hidden.data, group ? group.accountIds : null);
  if (shown.length === 0) {
    return (
      <EmptyState
        icon={EyeOff}
        title={t("unified.allHidden.title")}
        body={t("unified.allHidden.body")}
        action={{
          label: t("unified.allHidden.action"),
          onClick: () => {
            showAccountsPage({ kind: "list" });
          },
        }}
      />
    );
  }
  return <UnifiedPanes key={JSON.stringify(view)} view={view} accounts={accounts} name={name} platform={platform} />;
}
