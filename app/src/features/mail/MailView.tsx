import { useQueries } from "@tanstack/react-query";
import { FolderX, Inbox, ListChecks } from "lucide-react";
import { useEffect, useEffectEvent, useMemo, useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { EmptyState } from "../../components/EmptyState";
import { Welcome } from "./Welcome";
import { ErrorNotice } from "../../components/ErrorNotice";
import { ViewHeader } from "../../components/ViewHeader";
import {
  ipc,
  MAX_ACTION_MESSAGES,
  type Account,
  type Mailbox,
  type MessageId,
  type MessageListItem,
} from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";
import { openComposer } from "../../stores/composer";
import { boundShortcuts } from "../../stores/shortcuts";
import { useActionTarget } from "../../stores/actionTarget";
import { useUiStore, type ShownMailViewId, type View } from "../../stores/ui";
import { accountFill } from "../accounts/accountColor";
import { AccountErrorBanner, SyncStatus } from "../accounts/AccountStatus";
import { isFirstSyncRunning, useSelectedAccount } from "../accounts/selection";
import { ActionToolbar, QuickActions } from "../actions/ActionButtons";
import type { ActionHost } from "../actions/registry";
import { FolderActionOverlays, FolderMissingPrompt, useFolderActions } from "../actions/FolderActions";
import { tagsOf } from "../actions/keywords";
import { folderMenuScope, MessageMenu, type MenuScope } from "../actions/MessageMenu";
import {
  ACTION_IDS,
  ACTIONS,
  actionState,
  QUICK_ACTIONS,
  TOOLBAR_ACTIONS,
  type ActionContext,
} from "../actions/registry";
import { isEditableTarget, matchesShortcut, shortcutPlatform, type ShortcutPlatform } from "../actions/shortcuts";
import { useMarkReadOnOpen } from "../actions/useMarkReadOnOpen";
import { useAppInfo } from "../app-info/queries";
import { isMarkKind, useListSource, type ListKind } from "../comfort/listSources";
import type { MarkListKind } from "../comfort/queries";
import { ListResizeHandle } from "../layout/ListResizeHandle";
import { NoMessageSelected } from "../reading/NoMessageSelected";
import { usePreviewing } from "../reading/usePreviewing";
import { useReadingLayout } from "../layout/useReadingLayout";
import { mailboxesQueryKey, useMailboxes } from "../mailboxes/queries";
import { isRoleView, mailboxWithRole, type RoleView } from "../mailboxes/tree";
import { MessageList, NO_SELECTION, type ListSelection, type MessageListHandle, type RowMenuRequest } from "../messages/MessageList";
import { useMessagePages } from "../messages/queries";
import { ReadingPane, type FocusRequest } from "../reading/ReadingPane";
import { PinnedAttachmentsView } from "../reading/PinnedAttachmentsView";
import { AttachmentsView } from "../reading/AttachmentsView";
import { BlockedHiddenNote } from "../blocked/BlockedHiddenNote";
import { SavedSearchesView } from "../search/SavedSearchesView";
import { SearchArea } from "../search/SearchArea";
import { useSearchShortcut } from "../search/shortcut";
import { ConversationToggle } from "../threads/ConversationToggle";
import { useConversationsOn } from "../threads/preference";
import { useThreadSource } from "../threads/source";
import { ThreadPane } from "../threads/ThreadPane";
import { InboxScopeSwitcher } from "../unified/InboxScopeSwitcher";
import { UnifiedInbox, useUnifiedName } from "../unified/UnifiedInbox";
import { useAccountGroups, useHiddenAccounts } from "../unified/queries";
import { ScopedMarkView, ScopedSentView } from "../unified/AllAccountsViews";
import { coversScope, markScopeOf } from "../unified/scope";
// Stage 16: "Unread only" and "Unread first".
import { UnreadControls, unreadViewKey, useUnreadMode, useUnreadSource } from "../organize/unread";
import { unionIds, useDropRows } from "../organize/useDropRows";
// Stage 19: sorting, the table and the quick filter.
import { useListView } from "../views/useListView";
import { ListViewMenu } from "../views/ViewMenu";
import { QuickFilterToggle } from "../views/QuickFilterBar";
import type { InboxScope, UnifiedView } from "../../services/ipc";

export type MailViewState = Extract<View, { kind: "mail" } | { kind: "mailbox" }>;

/** Name of a folder as the UI shows it: the translated role, or the folder's own name. */
export function useMailboxLabel(): (mailbox: Mailbox) => string {
  const { t } = useTranslation();
  return (mailbox) => (mailbox.role === "custom" ? mailbox.name : t(`roles.${mailbox.role}`));
}

/** The account a view belongs to: its colour next to its name, never the colour alone. */
function AccountLine({ account }: { account: Account }) {
  const { t } = useTranslation();
  return (
    <>
      {/* Stage 23 (decision 0084): the account's square, as in the draft. */}
      <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-mark", accountFill(account.color))} />
      <span className="truncate">
        {account.displayName ? t("nav.account", { name: account.displayName, email: account.email }) : account.email}
      </span>
    </>
  );
}

/**
 * Stage 23, step 3 (decision 0084): the unread count next to the title in the toolbar: the
 * folder's, or the inboxes' of the accounts a unified view shows (as the sidebar counts them).
 * Null where a view has no such count (Later, Tasks, saved searches).
 */
function useViewUnread(unified: UnifiedView | null, accounts: readonly Account[], mailbox: Mailbox | undefined): number | null {
  const groups = useAccountGroups(unified?.kind === "group").data ?? [];
  const hidden = new Set(useHiddenAccounts(unified !== null).data ?? []);
  const ids = !unified
    ? []
    : unified.kind === "all"
      ? accounts.map((account) => account.id)
      : (groups.find((group) => group.id === unified.groupId)?.accountIds ?? []);
  const inboxes = useQueries({
    queries: ids.map((accountId) => ({ queryKey: mailboxesQueryKey(accountId), queryFn: () => ipc.listMailboxes(accountId) })),
  });
  if (unified) {
    return ids.reduce(
      (sum, id, index) => sum + (hidden.has(id) ? 0 : (inboxes[index]?.data?.find((inbox) => inbox.role === "inbox")?.unread ?? 0)),
      0,
    );
  }
  return mailbox ? mailbox.unread : null;
}

interface MailboxPanesProps {
  account: Account;
  mailbox: Mailbox;
  folderName: string;
  /** Stage 9: the sorted inbox, Later, Pinned or Tasks instead of the folder's pages. */
  kind?: ListKind;
}

interface SelectionBarProps {
  selection: ListSelection;
  total: number;
  ctx: ActionContext;
  /** The actions for the selection (stage 17: child rows among it act alone). */
  host: ActionHost;
  platform: ShortcutPlatform;
  list: RefObject<MessageListHandle | null>;
}

/**
 * Above the list while several messages are selected: how many, the actions for all of them, and
 * honestly how far "all" goes. The list is virtualized and loads pages on demand, so "all" is
 * everything loaded; loading the rest is a separate step, and one action takes at most
 * `MAX_ACTION_MESSAGES` messages.
 */
function SelectionBar({ selection, total, ctx, host, platform, list }: SelectionBarProps) {
  const { t, i18n } = useTranslation();
  const [loading, setLoading] = useState(false);
  const numbers = new Intl.NumberFormat(i18n.language);
  const count = selection.items.length;
  const max = MAX_ACTION_MESSAGES;
  const loadedSelectable = Math.min(selection.loaded, max);
  const allLoadedSelected = count >= loadedSelectable;
  const notLoaded = Math.max(0, total - selection.loaded);
  const limited = count >= max && (selection.hasMore || selection.loaded > max);
  return (
    <section
      aria-label={t("selection.label")}
      className="flex shrink-0 flex-col gap-2 border-b border-border bg-accent-soft px-4 py-2"
      data-testid="selection-bar"
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <p aria-live="polite" className="text-13 font-semibold text-primary" data-testid="selection-count">
          {t("selection.count", { count })}
        </p>
        {!allLoadedSelected && (
          <Button
            variant="quiet"
            onClick={() => {
              list.current?.selectAll();
            }}
            data-testid="select-all"
          >
            {t("selection.selectAll", { count: loadedSelectable })}
          </Button>
        )}
        {allLoadedSelected && selection.hasMore && selection.loaded < max && (
          <Button
            variant="quiet"
            disabled={loading}
            onClick={() => {
              setLoading(true);
              void list.current?.loadAndSelectAll().finally(() => {
                setLoading(false);
              });
            }}
            data-testid="select-all-folder"
          >
            {loading ? t("selection.loading") : t("selection.loadAll")}
          </Button>
        )}
        <Button
          variant="quiet"
          onClick={() => {
            list.current?.clearSelection();
          }}
          data-testid="clear-selection"
        >
          {t("selection.clear")}
        </Button>
      </div>
      <ActionToolbar ids={TOOLBAR_ACTIONS} ctx={ctx} host={host} platform={platform} label={t("actions.toolbar")} />
      {allLoadedSelected && selection.hasMore && notLoaded > 0 && !limited && (
        <p className="text-12 text-secondary" data-testid="selection-not-loaded">
          {t("selection.notLoaded", { count: notLoaded })}
        </p>
      )}
      {limited && (
        <p className="text-12 text-secondary" data-testid="selection-limit">
          {t("selection.limit", { max: numbers.format(max) })}
        </p>
      )}
    </section>
  );
}

/** Where the reading pane would be while several messages are selected. */
function SelectionSummary({ count, platform }: { count: number; platform: ShortcutPlatform }) {
  const { t } = useTranslation();
  return (
    <section
      aria-label={t("selection.summaryTitle", { count })}
      className="flex min-h-0 min-w-0 flex-1 flex-col bg-surface"
      data-testid="selection-summary"
    >
      <EmptyState
        icon={ListChecks}
        title={t("selection.summaryTitle", { count })}
        body={t("selection.summaryBody", { mod: platform === "macos" ? "⌘" : "Ctrl" })}
      />
    </section>
  );
}

/**
 * The list of one folder and, once a message is selected, the reading pane to its right. The
 * list then gets narrower and its rows two lines (docs/DESIGN.md). Focus: Enter or a double click
 * moves it into the pane, Escape there brings it back to the list, Escape in the list or the
 * pane's close button closes the pane.
 *
 * Actions (stage 4): quick actions in the rows, the toolbar of the reading pane, the selection
 * bar, drag and drop onto folders and the keyboard shortcuts all come from the action directory
 * (`actions/registry.ts`) and run through one action controller.
 */
function MailboxPanes({ account, mailbox, folderName, kind = "folder" }: MailboxPanesProps) {
  const { t } = useTranslation();
  // Decision 0039: conversation view (a stored preference, on by default) for folders and the
  // sorted inbox, also "Unread only" and "Unread first"; Later, Pinned and Tasks stay per message.
  const [selection, setSelection] = useState<ListSelection>(NO_SELECTION);
  // Stage 16: "Unread only" and "Unread first" replace the folder's or the inbox's own list.
  const unreadScope: InboxScope | null = kind === "folder" || kind === "inbox" ? { kind: "mailbox", mailboxId: mailbox.id } : null;
  const unreadMode = useUnreadMode(unreadScope && unreadViewKey(unreadScope));
  const conversations = useConversationsOn() && (kind === "folder" || kind === "inbox");
  // Stage 19 (decisions 0061, 0062): the view's order, its quick filter and the table.
  const listView = useListView({
    scope: unreadScope,
    keyBase: ["messages", account.id, mailbox.id],
    conversations,
    kept: selection.items,
    showRecipients: mailbox.role === "sent" || mailbox.role === "drafts",
    mixedAccounts: false,
  });
  const unreadSource = useUnreadSource(unreadScope, ["messages", account.id, mailbox.id], listView.active ? null : unreadMode, selection.items);
  const threadSource = useThreadSource(account.id, mailbox.id, conversations && kind === "folder" && !unreadSource && !listView.active);
  const comfortSource = useListSource(unreadSource || listView.active ? "folder" : kind, account, mailbox);
  const source = listView.source ?? unreadSource ?? threadSource ?? comfortSource;
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const mailboxes = useMailboxes(account.id).data;
  const pages = useMessagePages(account.id, mailbox.id, kind === "folder" && !conversations && !unreadSource && !listView.active);
  const push = useNoticeStore((state) => state.push);
  const [focusRequest, setFocusRequest] = useState<FocusRequest | null>(null);
  const list = useRef<MessageListHandle>(null);
  // Stage 21: the controller with moving and copying, shared with the context menu.
  const actions = useFolderActions({ account, mailbox, list, conversations });
  const { controller } = actions;
  // Stage 22 (decision 0081): rows dragged onto any folder in the column or the rail.
  const drops = useDropRows({ list, conversations });
  const hiddenIds = useMemo(() => unionIds(controller.hiddenIds, drops.hiddenIds), [controller.hiddenIds, drops.hiddenIds]);
  /** Stage 21 (decision 0064): the context menu asked for on rows of the list. */
  const [menu, setMenu] = useState<RowMenuRequest | null>(null);
  const single = selection.items.length === 1 ? (selection.items[0] ?? null) : null;
  // Stage 17 (decision 0049): a message of an unfolded conversation opens and acts alone; a
  // conversation row opens one message of its conversation (`ThreadPane`), and keeps it while it
  // stays selected (`rowKey`: its identity, which outlives a newer message becoming the row's).
  const singleIsChild = single !== null && selection.childIds.has(single.id);
  const rowKey = selection.keys[0] ?? "";
  const conversationOpen = single !== null && conversations && !singleIsChild;
  const [shown, setShown] = useState<{ key: string; message: MessageListItem } | null>(null);
  const opened = !single ? null : conversationOpen ? (shown?.key === rowKey ? shown.message : null) : single;
  const open = selection.items.length > 0;
  // Stage 8: the reading pane right, below or in place of the list.
  const layout = useReadingLayout(list);

  // In place of the list a message is open only once opened, not while the cursor rests on it.
  // Stage 17: what is marked read is the message the pane shows, not the whole conversation.
  // Stage 18 (decision 0054): not the top message the list opened by itself, until the user
  // selects it or works with it.
  const preview = usePreviewing(selection);
  useMarkReadOnOpen(account, layout.paneShown(open) && !preview.previewing ? opened : null, controller, { exact: conversations });

  const ctxFor = (items: readonly MessageListItem[]): ActionContext => ({
    account,
    mailbox,
    items,
    canUndo: controller.canUndo,
  });
  /** One message with the folder it lies in (a child row, the reading pane's message). */
  const ctxForMessage = (message: MessageListItem): ActionContext => ({
    ...ctxFor([message]),
    mailbox: mailboxes?.find((m) => m.id === message.mailboxId) ?? mailbox,
  });
  // What a shortcut acts on: the selection, else the row with the cursor; child rows act alone.
  const targets = selection.items.length > 0 ? selection.items : selection.cursor ? [selection.cursor] : [];
  const targetChildren: ReadonlySet<MessageId> =
    selection.items.length > 0 ? selection.childIds : selection.cursorIsChild && selection.cursor ? new Set([selection.cursor.id]) : new Set();
  const targetHost = controller.hostFor(targetChildren);
  const targetCtx = targets.length === 1 && targetChildren.size === 1 && targets[0] ? ctxForMessage(targets[0]) : ctxFor(targets);
  // Stage 8: the command palette runs the same actions on the same messages.
  const pickerOpen = Boolean(controller.overlay || controller.comfort.snoozeItems || controller.comfort.followUpItems);
  useActionTarget({ ctx: pickerOpen ? null : targetCtx, host: targetHost });

  const onShortcut = useEffectEvent((event: KeyboardEvent) => {
    if (event.defaultPrevented || event.repeat || isEditableTarget(event.target)) return;
    if (controller.overlay || controller.comfort.snoozeItems || controller.comfort.followUpItems) return;
    const ctx = targetCtx;
    for (const id of ACTION_IDS) {
      const definition = ACTIONS[id];
      // Stage 8: the user's assignment, read at the key press so a change works at once.
      if (!boundShortcuts(id, platform).some((shortcut) => matchesShortcut(event, shortcut, platform))) continue;
      if (definition.listOnly && !list.current?.contains(event.target instanceof Node ? event.target : null)) continue;
      const state = actionState(definition, ctx);
      if (state.kind === "unavailable") continue;
      event.preventDefault();
      // Same explanation as the disabled buttons: why nothing happens, and where to change it.
      if (state.kind === "readOnly") push({ kind: "readOnly", accountId: account.id });
      else definition.run(ctx, targetHost);
      return;
    }
  });
  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      onShortcut(event);
    };
    window.addEventListener("keydown", listener);
    return () => {
      window.removeEventListener("keydown", listener);
    };
  }, []);

  const loaded = (source?.query.data ?? pages.data)?.pages.flatMap((page) => page.items) ?? [];
  const byId = new Map(loaded.map((item) => [item.id, item]));
  const focusList = () => {
    list.current?.focus();
  };
  /** Stage 21: the menu acts like the selection bar: child rows alone, a single one in its folder. */
  const menuScope = (request: RowMenuRequest): MenuScope => {
    const one = request.items.length === 1 && request.childIds.size === 1 ? request.items[0] : undefined;
    return folderMenuScope(actions, one ? ctxForMessage(one) : ctxFor(request.items), request.childIds);
  };

  return (
    <div className={layout.container} data-reading-pane={layout.position}>
      {/* Floating or in a layer of their own, so their place here only matters for the commit:
          before the list, a dialog closes while the list's handle is still attached, and its
          focus goes back to the list. */}
      <FolderActionOverlays actions={actions} mailboxes={mailboxes ?? []} loaded={loaded} focusList={focusList} />
      <div {...layout.listColumn(open)}>
        <FolderMissingPrompt actions={actions} mailboxes={mailboxes ?? []} focusList={focusList} />
        {selection.items.length > 1 && (
          <SelectionBar
            selection={selection}
            total={mailbox.total}
            ctx={ctxFor(selection.items)}
            host={controller.hostFor(selection.childIds)}
            platform={platform}
            list={list}
          />
        )}
        {listView.bar}
        {source?.controls}
        {/* Stage 16 (decision 0036): mail of blocked senders the inbox leaves out. */}
        {kind === "inbox" && <BlockedHiddenNote scope={{ kind: "mailbox", mailboxId: mailbox.id }} accountId={account.id} />}
        <MessageList
          account={account}
          mailbox={mailbox}
          folderName={folderName}
          showRecipients={mailbox.role === "sent" || mailbox.role === "drafts"}
          platform={platform}
          hiddenIds={hiddenIds}
          source={source}
          handle={list}
          // Stage 9 (decision 0028): notifications and AI sources select their message here. Later,
          // Pinned and Tasks show the inbox's messages but are not the inbox: no folder scope.
          selectScope={{
            accountId: account.id,
            mailboxId: kind === "folder" || kind === "inbox" ? mailbox.id : null,
            inbox: kind === "inbox",
          }}
          onSelectedById={() => {
            layout.onOpen();
          }}
          onSelectionChange={(next) => {
            setSelection(next);
            const focusPane = layout.onSelectionChange(next);
            // A request to focus belongs to the message it was made for. In place of the list the
            // next message after an action gets one (the hidden list cannot keep the focus).
            const only = next.items.length === 1 ? next.items[0]?.id : undefined;
            setFocusRequest((request) =>
              request && request.messageId === only
                ? request
                : focusPane && only !== undefined
                  ? { messageId: only, seq: (request?.seq ?? 0) + 1 }
                  : null,
            );
          }}
          onOpen={(id) => {
            // Stage 5: a draft opens in the composer (also one written by another program).
            if (byId.get(id)?.draft && mailbox.role === "drafts") {
              openComposer({ kind: "message", messageId: id });
              return;
            }
            layout.onOpen();
            preview.engage();
            setFocusRequest((request) => ({ messageId: id, seq: (request?.seq ?? 0) + 1 }));
          }}
          // Stage 18 (task F): opening a folder shows its top message (not in place of the list).
          autoSelectFirst={layout.autoSelectFirst}
          renderQuickActions={(item, _selected, child) => (
            <QuickActions
              ids={QUICK_ACTIONS}
              ctx={child ? ctxForMessage(item) : ctxFor([item])}
              host={child ? controller.hostFor(new Set([item.id])) : controller.host}
              platform={platform}
            />
          )}
          onDragRows={drops.onDragRows}
          onDragEnd={drops.onDragEnd}
          table={listView.table}
          onRowMenu={setMenu}
        />
      </div>
      {/* Stage 18: the border between list and pane can be dragged. */}
      <ListResizeHandle layout={layout} />
      {/* Stage 18 (task F): with nothing selected the pane's place stays, with a quiet note. */}
      {layout.placeholderShown(open) && <NoMessageSelected />}
      {/* A press, a key or the focus in the pane: the user works with the message (decision 0054). */}
      <div className="contents" {...preview.paneProps}>
        {!layout.paneShown(open) ? null : single && conversationOpen ? (
          <ThreadPane
            key={rowKey}
            account={account}
            item={single}
            ident={rowKey}
            focusRequest={focusRequest}
            // The toolbar acts on the open message alone, with its own folder's actions; the row's
            // actions act on the whole conversation.
            toolbarFor={(message) => (
              <ActionToolbar
                ids={TOOLBAR_ACTIONS}
                ctx={ctxForMessage(message)}
                host={controller.messageHost}
                platform={platform}
                label={t("actions.toolbar")}
              />
            )}
            onShown={(message) => {
              setShown({ key: rowKey, message });
            }}
            onShowMessage={(messageId) => {
              list.current?.showInConversation(rowKey, messageId);
            }}
            onEscape={() => {
              layout.backToList(focusList);
            }}
            onClose={() => {
              layout.close(() => list.current?.clearSelection());
            }}
          />
        ) : single ? (
          <ReadingPane
            key={single.id}
            account={account}
            messageId={single.id}
            mailboxId={single.mailboxId}
            draft={single.draft}
            focusRequest={focusRequest}
            toolbar={
              <ActionToolbar
                ids={TOOLBAR_ACTIONS}
                ctx={singleIsChild ? ctxForMessage(single) : ctxFor([single])}
                host={singleIsChild ? controller.hostFor(new Set([single.id])) : controller.host}
                platform={platform}
                label={t("actions.toolbar")}
              />
            }
            onEscape={() => {
              layout.backToList(focusList);
            }}
            onClose={() => {
              layout.close(() => list.current?.clearSelection());
            }}
          />
        ) : (
          selection.items.length > 1 && <SelectionSummary count={selection.items.length} platform={platform} />
        )}
      </div>
      {menu && (
        <MessageMenu
          request={menu}
          scopes={[menuScope(menu)]}
          platform={platform}
          knownTags={tagsOf([...loaded, ...menu.items])}
          onClose={() => {
            setMenu(null);
          }}
        />
      )}
    </div>
  );
}

/**
 * What a mail view lists: the folder with a role, or the marked messages of the account. Every
 * view of the rail has one, except the two with panes of their own (`null`).
 */
function mailViewList(mailView: ShownMailViewId): { kind: "role"; role: RoleView } | { kind: "mark"; mark: MarkListKind } | null {
  switch (mailView) {
    case "inbox":
    case "sent":
    case "drafts":
    case "archive":
    case "trash":
      return { kind: "role", role: mailView };
    case "later":
    case "waiting":
    case "pinned":
    case "tasks":
    case "readingList":
    case "flagged":
      return { kind: "mark", mark: mailView };
    // Stage 17 (decision 0050): the contacts are rendered by `AppShell` as a view of their own.
    case "pinnedAttachments":
    case "attachments":
    case "savedViews":
    case "contacts":
      return null;
  }
}

interface MailBodyProps {
  view: MailViewState;
  account: Account;
}

/** What a mail view shows for one account: its folder's messages, or why there are none. */
function MailBody({ view, account }: MailBodyProps) {
  const { t } = useTranslation();
  const mailboxes = useMailboxes(account.id);
  const label = useMailboxLabel();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const accountName = account.displayName || account.email;

  // Stage 16 (decision 0036): pinned attachments of the account, like the reading list.
  if (view.kind === "mail" && view.mailView === "pinnedAttachments") {
    return <PinnedAttachmentsView scope={{ kind: "account", accountId: account.id }} />;
  }
  // Stage 20 (decision 0060): every attachment, across the accounts (it filters by account).
  if (view.kind === "mail" && view.mailView === "attachments") {
    return <AttachmentsView />;
  }
  const list = view.kind === "mail" ? mailViewList(view.mailView) : null;
  // The saved searches have a pane of their own, which `MailView` shows instead of this.
  if (view.kind === "mail" && !list) return null;
  const roleView = list?.kind === "role" ? list.role : null;
  // Stage 9: Later, Pinned and Tasks show the marked messages of the account's inbox.
  const markView = list?.kind === "mark" ? list.mark : null;
  if (mailboxes.isPending) return null;
  if (mailboxes.isError) {
    return (
      <div className="px-6 py-4">
        <ErrorNotice
          context={t("sidebar.loadFailed")}
          error={mailboxes.error}
          onRetry={() => {
            void mailboxes.refetch();
          }}
        />
      </div>
    );
  }

  const mailbox =
    view.kind === "mailbox"
      ? mailboxes.data.find((m) => m.id === view.mailboxId)
      : markView
        ? mailboxWithRole(mailboxes.data, "inbox")
        : roleView && mailboxWithRole(mailboxes.data, roleView);

  if (!mailbox) {
    if (mailboxes.data.length === 0 && isFirstSyncRunning(account.status)) {
      return (
        <EmptyState
          icon={Inbox}
          title={t("list.firstSync.title")}
          body={t("list.firstSync.body", { account: accountName })}
        />
      );
    }
    if (!roleView && !markView) {
      return <EmptyState icon={FolderX} title={t("mail.folderGone.title")} body={t("mail.folderGone.body")} />;
    }
    const role = t(`roles.${roleView ?? "inbox"}`);
    return (
      <EmptyState
        icon={FolderX}
        title={t("mail.noRoleFolder.title", { role })}
        body={t("mail.noRoleFolder.body", { role, account: accountName })}
        action={{
          label: t("mail.noRoleFolder.action"),
          onClick: () => {
            showAccountsPage({ kind: "account", accountId: account.id });
          },
        }}
      />
    );
  }

  const kind: ListKind = markView ?? (mailbox.role === "inbox" ? "inbox" : "folder");
  return (
    <MailboxPanes
      key={`${String(mailbox.id)}-${kind}`}
      account={account}
      mailbox={mailbox}
      folderName={markView ? t(`nav.${markView}`) : label(mailbox)}
      kind={kind}
    />
  );
}

interface ScopedViewProps {
  view: UnifiedView;
  mailView: ShownMailViewId;
  accounts: readonly Account[];
  platform: ShortcutPlatform;
  name: string;
}

/**
 * Stage 23 (decision 0087): a view of "All accounts" (other than the inbox) over the unified
 * scope: the marked messages, the pinned attachments or Sent of every visible account (or of a
 * group's), mixed by account.
 */
function ScopedView({ view, mailView, accounts, platform, name }: ScopedViewProps) {
  if (mailView === "pinnedAttachments") return <PinnedAttachmentsView scope={markScopeOf(view)} />;
  if (mailView === "sent") return <ScopedSentView view={view} accounts={accounts} platform={platform} name={name} />;
  if (isMarkKind(mailView)) return <ScopedMarkView key={mailView} kind={mailView} view={view} accounts={accounts} platform={platform} name={name} />;
  return null;
}

interface MailViewProps {
  view: MailViewState;
}

/**
 * A mail view: header with the folder and its account, then the message list. Until the
 * unified inbox (stage 7) every view shows the folder of the selected account.
 */
export function MailView({ view }: MailViewProps) {
  const { t } = useTranslation();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const { accounts, account } = useSelectedAccount();
  const mailboxes = useMailboxes(view.kind === "mailbox" ? view.accountId : null);
  const label = useMailboxLabel();
  // Stage 7: search, the unified inbox and its switcher in the header.
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  useSearchShortcut(platform);
  const unified = useUiStore((state) => state.unifiedInbox);
  const accountList = accounts.data ?? [];
  const inboxView = view.kind === "mail" && view.mailView === "inbox";
  // Decision 0039: conversations for folders, inboxes and the unified inbox (not Later, Pinned, Tasks).
  const conversationsApply = view.kind === "mailbox" || isRoleView(view.mailView);
  // Stage 23 (decision 0087): while the unified scope is on, the views of "All accounts" (Later,
  // Flagged, Sent, …) cover its accounts like the inbox does.
  const scopedView = view.kind === "mail" && account && coversScope(view.mailView) ? unified : null;
  const unifiedView = inboxView ? scopedView : null;
  const unifiedName = useUnifiedName(scopedView ?? { kind: "all" });
  const roleMailboxes = useMailboxes(scopedView ? null : account?.id).data ?? [];
  const searchContext = {
    accountId: scopedView ? null : (account?.id ?? null),
    mailboxId:
      view.kind === "mailbox"
        ? view.mailboxId
        : !scopedView && isRoleView(view.mailView)
          ? (mailboxWithRole(roleMailboxes, view.mailView)?.id ?? null)
          : null,
    groupId: scopedView?.kind === "group" ? scopedView.groupId : null,
  };

  const folder = view.kind === "mailbox" ? mailboxes.data?.find((m) => m.id === view.mailboxId) : undefined;
  // Stage 16: the view "Unread only" and "Unread first" apply to (folders, inboxes, unified views).
  const roleMailbox = view.kind === "mail" && !scopedView && isRoleView(view.mailView) ? mailboxWithRole(roleMailboxes, view.mailView) : undefined;
  const unreadScope: InboxScope | null = unifiedView
    ? { kind: "unified", view: unifiedView }
    : folder
      ? { kind: "mailbox", mailboxId: folder.id }
      : roleMailbox
        ? { kind: "mailbox", mailboxId: roleMailbox.id }
        : null;
  const shownRole = (folder ?? roleMailbox)?.role;
  const unread = useViewUnread(unifiedView, accountList, view.kind === "mail" && !isRoleView(view.mailView) ? undefined : (folder ?? roleMailbox));
  const sentOrDrafts = !unifiedView && (shownRole === "sent" || shownRole === "drafts");
  const title = unifiedView
    ? unifiedName
    : view.kind === "mail"
      ? t(`nav.${view.mailView}`)
      : folder
        ? label(folder)
        : t("nav.folders");

  let body;
  if (accounts.isPending) {
    body = null;
  } else if (accounts.isError) {
    body = (
      <div className="px-6 py-4">
        <ErrorNotice
          context={t("settings.accounts.loadFailed")}
          error={accounts.error}
          onRetry={() => {
            void accounts.refetch();
          }}
        />
      </div>
    );
  } else if (!account) {
    // Design pass 23.11.2026: the first start shows the wordmark and the one next step.
    body = (
      <Welcome
        onAdd={() => {
          showAccountsPage({ kind: "add" });
        }}
      />
    );
  } else if (view.kind === "mail" && view.mailView === "savedViews") {
    // Stage 7: the saved searches, or the results of one of them over all accounts.
    body = (
      <SearchArea context={searchContext} platform={platform}>
        <SavedSearchesView savedSearchId={view.savedSearchId ?? null} accounts={accountList} platform={platform} />
      </SearchArea>
    );
  } else if (unifiedView) {
    body = (
      <SearchArea context={searchContext} platform={platform}>
        <UnifiedInbox view={unifiedView} accounts={accountList} platform={platform} />
      </SearchArea>
    );
  } else if (scopedView && view.kind === "mail") {
    body = (
      <SearchArea context={searchContext} platform={platform}>
        <ScopedView view={scopedView} mailView={view.mailView} accounts={accountList} platform={platform} name={title} />
      </SearchArea>
    );
  } else {
    body = (
      <SearchArea context={searchContext} platform={platform}>
        <AccountErrorBanner account={account} />
        <MailBody view={view} account={account} />
      </SearchArea>
    );
  }
  // Stage 23, step 3 (decision 0084): the head stands in the toolbar over the list: the title,
  // the inbox's scope as its menu ("All inboxes ▾"), the account where several exist, the unread
  // count; the list's controls at the segment's end. Search, New mail and the AI panel's toggle
  // are the toolbar's own.
  const switcher = inboxView && account && (accountList.length > 1 || unifiedView);
  const after = (
    <>
      {switcher && <InboxScopeSwitcher accounts={accountList} account={account} inTitle />}
      {/* Stage 23 (decision 0087): a view of "All accounts" names its scope, not one account. */}
      {!switcher && scopedView && accountList.length > 1 && (
        <span className="min-w-0 truncate text-12 text-tertiary" data-testid="view-scope">
          {scopedView.kind === "all" ? t("nav.allAccounts") : unifiedName}
        </span>
      )}
      {!switcher && !scopedView && account && accountList.length > 1 && (
        <span className="flex min-w-0 items-center gap-2 text-12 text-tertiary" data-testid="view-account">
          <AccountLine account={account} />
        </span>
      )}
      {unread !== null && unread > 0 && (
        <span
          className="shrink-0 text-12 whitespace-nowrap text-tertiary tabular-nums @max-[70rem]/toolbar:hidden group-data-[narrow]/toolbarlist:hidden"
          data-testid="view-unread"
        >
          {t("toolbar.unread", { count: unread })}
        </span>
      )}
    </>
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ViewHeader title={title} after={after}>
        {account && accounts.data && <SyncStatus accounts={accounts.data} account={account} />}
        <div className="flex shrink-0 items-center gap-1">
          {/* Stage 23: the quick filter folds into this button (its bar shows above the list). */}
          {account && unreadScope && <QuickFilterToggle viewKey={unreadViewKey(unreadScope)} />}
          {/* Stage 19 (decision 0061): order, presentation and table columns of the view. */}
          {account && unreadScope && (
            <ListViewMenu
              viewKey={unreadViewKey(unreadScope)}
              showRecipients={sentOrDrafts}
              mixedAccounts={Boolean(unifiedView) && accountList.length > 1}
            />
          )}
          {account && unreadScope && <UnreadControls viewKey={unreadViewKey(unreadScope)} />}
          {account && conversationsApply && <ConversationToggle always={!unreadScope} />}
        </div>
      </ViewHeader>
      {body}
    </div>
  );
}
