import { EyeOff, FolderX, Inbox } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { Account, AccountColor, InboxScope, MessageId, MessageListItem, UnifiedView } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import type { ShortcutPlatform } from "../actions/shortcuts";
import type { MessageListSource } from "../messages/listSource";
import { MessageList, NO_SELECTION, type ListSelection, type MessageListHandle } from "../messages/MessageList";
import { MIXED_LIST_SCOPE } from "../../stores/listSelect";
import { useUnifiedInboxSource } from "../comfort/listSources";
import { useReadingLayout } from "../layout/useReadingLayout";
import { MixedReading } from "./MixedReading";
import type { FocusRequest } from "../reading/ReadingPane";
import { useAccountGroups, useHiddenAccounts } from "./queries";
import { accountShortNames } from "../accounts/accountName";
import { unreadViewKey, useUnreadMode, useUnreadSource } from "../organize/unread";

/** The accounts a unified view mixes: visible ones, of the group if it is a group view. */
export function visibleAccounts(
  accounts: readonly Account[],
  hidden: readonly string[],
  memberIds: readonly string[] | null,
): Account[] {
  return accounts.filter((account) => !hidden.includes(account.id) && (memberIds === null || memberIds.includes(account.id)));
}

interface PanesProps {
  view: UnifiedView;
  accounts: readonly Account[];
  name: string;
  platform: ShortcutPlatform;
}

/**
 * The list of a unified view and the reading pane: the sorted inbox (stage 9: blocks, bundles,
 * hidden snoozes) over every shown account, each row in its account's colour (the avatar ring),
 * opened with the account it belongs to. The conversation view does not apply to inboxes: the
 * core has no thread variant of the sorted inbox.
 */
function UnifiedPanes({ view, accounts, name, platform }: PanesProps) {
  const { t } = useTranslation();
  const [selection, setSelection] = useState<ListSelection>(NO_SELECTION);
  const [focusRequest, setFocusRequest] = useState<FocusRequest | null>(null);
  const list = useRef<MessageListHandle>(null);
  const colors = useMemo(() => new Map(accounts.map((account) => [account.id, account.color] as const)), [accounts]);
  const accountColor = (item: MessageListItem): AccountColor => colors.get(item.accountId) ?? 0;
  // Stage 8: with more than one account, each row names its account next to the colour.
  const names = useMemo(() => (accounts.length > 1 ? accountShortNames(accounts) : null), [accounts]);
  // Stage 16: "Unread only" and "Unread first" over the same accounts.
  const unreadScope: InboxScope = { kind: "unified", view };
  const unreadMode = useUnreadMode(unreadViewKey(unreadScope));
  const unread = useUnreadSource(unreadScope, ["unified", view], unreadMode, selection.items);
  const sorted = useUnifiedInboxSource(view, !unread);
  const source: MessageListSource = {
    empty: { icon: Inbox, title: t("unified.empty.title"), body: t("unified.empty.body") },
    ...(unread ?? sorted),
    accountColor,
    ...(names && { accountName: (item: MessageListItem) => names.get(item.accountId) }),
  };
  const query = source.query;
  const single = selection.items.length === 1 ? (selection.items[0] ?? null) : null;
  const account = single ? accounts.find((a) => a.id === single.accountId) : undefined;
  // Stage 8: the reading pane right, below or in place of the list.
  const layout = useReadingLayout(list);
  const open = Boolean(single && account);

  if (query.isError && !query.data) {
    return (
      <div className="px-6 py-4">
        <ErrorNotice
          context={t("unified.loadFailed")}
          error={query.error}
          onRetry={() => {
            void query.refetch();
          }}
        />
      </div>
    );
  }

  return (
    <div className={layout.container} data-testid="unified-inbox" data-reading-pane={layout.position}>
      <div {...layout.listColumn(open)}>
        <MessageList
          source={source}
          folderName={name}
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
            setFocusRequest((request) => ({ messageId: id, seq: (request?.seq ?? 0) + 1 }));
          }}
        />
      </div>
      {single && account && layout.paneShown(open) && (
        <MixedReading
          key={single.id}
          account={account}
          item={single}
          focusRequest={focusRequest}
          platform={platform}
          list={list}
          layout={layout}
        />
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
