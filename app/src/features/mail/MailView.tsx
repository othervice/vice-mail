import { FolderX, Hourglass, Inbox, ListChecks, Search, SquarePen } from "lucide-react";
import { useEffect, useEffectEvent, useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { cx } from "../../components/cx";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import { ViewHeader } from "../../components/ViewHeader";
import {
  MARK_READ_DELAY_MS,
  MAX_ACTION_MESSAGES,
  type Account,
  type Mailbox,
  type MessageId,
  type MessageListItem,
} from "../../services/ipc";
import { useDragStore } from "../../stores/drag";
import { useNoticeStore } from "../../stores/notices";
import { useUiStore, type View } from "../../stores/ui";
import { accountFill } from "../accounts/accountColor";
import { AccountErrorBanner, SyncStatus } from "../accounts/AccountStatus";
import { isFirstSyncRunning, useSelectedAccount } from "../accounts/selection";
import { ActionToolbar, QuickActions } from "../actions/ActionButtons";
import { MissingMailboxPrompt } from "../actions/MissingMailboxPrompt";
import { MovePicker, TagEditor } from "../actions/Pickers";
import { useReadingPreferences } from "../actions/queries";
import {
  ACTION_IDS,
  ACTIONS,
  actionState,
  QUICK_ACTIONS,
  TOOLBAR_ACTIONS,
  type ActionContext,
} from "../actions/registry";
import { isEditableTarget, matchesShortcut, shortcutPlatform, type ShortcutPlatform } from "../actions/shortcuts";
import { useActionController, type ActionController } from "../actions/useActionController";
import { useAppInfo } from "../app-info/queries";
import { useWindowDragRegion } from "../app-info/useWindowDragRegion";
import { useMailboxes } from "../mailboxes/queries";
import { isRoleView, mailboxWithRole } from "../mailboxes/tree";
import { MessageList, NO_SELECTION, type ListSelection, type MessageListHandle } from "../messages/MessageList";
import { useMessagePages } from "../messages/queries";
import { ReadingPane, type FocusRequest } from "../reading/ReadingPane";

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
      <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-full", accountFill(account.color))} />
      <span className="truncate">
        {account.displayName ? t("nav.account", { name: account.displayName, email: account.email }) : account.email}
      </span>
    </>
  );
}

interface MailboxPanesProps {
  account: Account;
  mailbox: Mailbox;
  folderName: string;
}

/**
 * "Mark as read when opened" (Settings, Reading): right away, after `MARK_READ_DELAY_MS` of the
 * message staying open (a change of selection before cancels it), or never. Only for accounts
 * with writes on, and once per opening: a message the user marks unread while it is open stays
 * unread. An ordinary `markRead` action, without an undo notice.
 */
function useMarkReadOnOpen(account: Account, item: MessageListItem | null, controller: ActionController) {
  const preferences = useReadingPreferences();
  const mode = preferences.data?.markReadOnOpen;
  const handled = useRef<MessageId | null>(null);
  const id = item?.id ?? null;
  const unread = item?.unread ?? false;
  const writes = account.writesEnabled;
  const markRead = useEffectEvent(() => {
    if (item) void controller.apply({ type: "markRead" }, [item], { silent: true, errorContext: "markRead" });
  });
  useEffect(() => {
    if (id === null) {
      handled.current = null;
      return undefined;
    }
    if (handled.current === id) return undefined;
    if (!unread) {
      handled.current = id;
      return undefined;
    }
    if (!mode || mode === "never" || !writes) return undefined;
    if (mode === "immediately") {
      handled.current = id;
      markRead();
      return undefined;
    }
    const timer = window.setTimeout(() => {
      handled.current = id;
      markRead();
    }, MARK_READ_DELAY_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [id, unread, mode, writes]);
}

interface SelectionBarProps {
  selection: ListSelection;
  total: number;
  ctx: ActionContext;
  controller: ActionController;
  platform: ShortcutPlatform;
  list: RefObject<MessageListHandle | null>;
}

/**
 * Above the list while several messages are selected: how many, the actions for all of them, and
 * honestly how far "all" goes. The list is virtualized and loads pages on demand, so "all" is
 * everything loaded; loading the rest is a separate step, and one action takes at most
 * `MAX_ACTION_MESSAGES` messages.
 */
function SelectionBar({ selection, total, ctx, controller, platform, list }: SelectionBarProps) {
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
      <ActionToolbar ids={TOOLBAR_ACTIONS} ctx={ctx} host={controller.host} platform={platform} label={t("actions.toolbar")} />
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
function MailboxPanes({ account, mailbox, folderName }: MailboxPanesProps) {
  const { t } = useTranslation();
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const mailboxes = useMailboxes(account.id).data;
  const pages = useMessagePages(account.id, mailbox.id);
  const label = useMailboxLabel();
  const push = useNoticeStore((state) => state.push);
  const startDrag = useDragStore((state) => state.start);
  const endDrag = useDragStore((state) => state.end);
  const [selection, setSelection] = useState<ListSelection>(NO_SELECTION);
  const [focusRequest, setFocusRequest] = useState<FocusRequest | null>(null);
  const list = useRef<MessageListHandle>(null);
  const controller = useActionController({ account, mailbox, list });
  const single = selection.items.length === 1 ? (selection.items[0] ?? null) : null;
  const open = selection.items.length > 0;

  useMarkReadOnOpen(account, single, controller);

  const ctxFor = (items: readonly MessageListItem[]): ActionContext => ({
    account,
    mailbox,
    items,
    canUndo: controller.canUndo,
  });
  // What a shortcut acts on: the selection, else the row with the cursor.
  const targets = selection.items.length > 0 ? selection.items : selection.cursor ? [selection.cursor] : [];

  const onShortcut = useEffectEvent((event: KeyboardEvent) => {
    if (event.defaultPrevented || event.repeat || isEditableTarget(event.target)) return;
    if (controller.overlay) return;
    const ctx = ctxFor(targets);
    for (const id of ACTION_IDS) {
      const definition = ACTIONS[id];
      if (!definition.shortcuts[platform].some((shortcut) => matchesShortcut(event, shortcut, platform))) continue;
      if (definition.listOnly && !list.current?.contains(event.target instanceof Node ? event.target : null)) continue;
      const state = actionState(definition, ctx);
      if (state.kind === "unavailable") continue;
      event.preventDefault();
      // Same explanation as the disabled buttons: why nothing happens, and where to change it.
      if (state.kind === "readOnly") push({ kind: "readOnly", accountId: account.id });
      else definition.run(ctx, controller.host);
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

  const loaded = pages.data?.pages.flatMap((page) => page.items) ?? [];
  const byId = new Map(loaded.map((item) => [item.id, item]));
  const fresh = (ids: readonly MessageId[]) => ids.flatMap((id) => byId.get(id) ?? []);
  const { overlay } = controller;
  const focusList = () => {
    list.current?.focus();
  };

  return (
    <div className="flex min-h-0 flex-1">
      <div
        className={cx(
          "flex min-h-0 min-w-0 flex-col",
          open ? "w-(--ui-list-width-reading) shrink-0 border-e border-border" : "flex-1",
        )}
      >
        {controller.missing && (
          <MissingMailboxPrompt
            key={`${controller.missing.role}-${String(controller.missing.items.length)}`}
            account={account}
            mailboxes={mailboxes ?? []}
            missing={controller.missing}
            onChoose={controller.chooseFolderForRole}
            onCreate={controller.createFolderForRole}
            onCancel={() => {
              controller.cancelMissing();
              focusList();
            }}
          />
        )}
        {selection.items.length > 1 && (
          <SelectionBar
            selection={selection}
            total={mailbox.total}
            ctx={ctxFor(selection.items)}
            controller={controller}
            platform={platform}
            list={list}
          />
        )}
        <MessageList
          account={account}
          mailbox={mailbox}
          folderName={folderName}
          showRecipients={mailbox.role === "sent" || mailbox.role === "drafts"}
          platform={platform}
          hiddenIds={controller.hiddenIds}
          handle={list}
          onSelectionChange={(next) => {
            setSelection(next);
            // A request to focus belongs to the message it was made for.
            const only = next.items.length === 1 ? next.items[0]?.id : undefined;
            setFocusRequest((request) => (request && request.messageId === only ? request : null));
          }}
          onOpen={(id) => {
            setFocusRequest((request) => ({ messageId: id, seq: (request?.seq ?? 0) + 1 }));
          }}
          renderQuickActions={(item) => (
            <QuickActions ids={QUICK_ACTIONS} ctx={ctxFor([item])} host={controller.host} platform={platform} />
          )}
          onDragRows={(items) => {
            startDrag({
              accountId: account.id,
              sourceMailboxId: mailbox.id,
              count: items.length,
              drop: (target) => {
                void controller.apply({ type: "move", target: target.id }, items, { detail: label(target) });
              },
            });
          }}
          onDragEnd={endDrag}
        />
      </div>
      {single ? (
        <ReadingPane
          key={single.id}
          account={account}
          messageId={single.id}
          focusRequest={focusRequest}
          toolbar={
            <ActionToolbar
              ids={TOOLBAR_ACTIONS}
              ctx={ctxFor([single])}
              host={controller.host}
              platform={platform}
              label={t("actions.toolbar")}
            />
          }
          onEscape={focusList}
          onClose={() => {
            list.current?.clearSelection();
          }}
        />
      ) : (
        selection.items.length > 1 && <SelectionSummary count={selection.items.length} platform={platform} />
      )}
      {overlay?.kind === "confirmDelete" && (
        <ConfirmDialog
          title={t("actions.confirmDelete.title", { count: overlay.items.length })}
          body={t("actions.confirmDelete.body")}
          confirmLabel={t("actions.confirmDelete.confirm")}
          cancelLabel={t("actions.confirmDelete.cancel")}
          onConfirm={() => {
            const { items } = overlay;
            controller.closeOverlay();
            void controller.apply({ type: "deletePermanently" }, items);
          }}
          onCancel={controller.closeOverlay}
          focusAfterConfirm={focusList}
          data-testid="confirm-delete"
        />
      )}
      {overlay?.kind === "move" && (
        <MovePicker
          count={overlay.ids.length}
          mailboxes={mailboxes ?? []}
          current={mailbox}
          onChoose={(target) => {
            const items = fresh(overlay.ids);
            controller.closeOverlay();
            void controller.apply({ type: "move", target: target.id }, items, { detail: label(target) });
          }}
          onClose={controller.closeOverlay}
          restoreFocus={focusList}
        />
      )}
      {overlay?.kind === "tags" && (
        <TagEditor
          items={fresh(overlay.ids)}
          known={loaded}
          onAdd={(tag) =>
            controller.apply({ type: "addKeyword", keyword: tag }, fresh(overlay.ids), {
              detail: tag,
              ownErrors: ["invalid_keyword"],
            })
          }
          onRemove={(tag) =>
            controller.apply(
              { type: "removeKeyword", keyword: tag },
              fresh(overlay.ids).filter((item) => item.keywords.some((k) => k.toLowerCase() === tag.toLowerCase())),
              { detail: tag },
            )
          }
          onClose={controller.closeOverlay}
          restoreFocus={focusList}
        />
      )}
    </div>
  );
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

  const roleView = view.kind === "mail" && isRoleView(view.mailView) ? view.mailView : null;
  if (view.kind === "mail" && !roleView) {
    // Views without a function yet (Later, Tasks, Pinned, Saved views) say so honestly.
    const name = t(`nav.${view.mailView}`);
    return <EmptyState icon={Hourglass} title={t("mail.notYet.title")} body={t("mail.notYet.body", { view: name })} />;
  }
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
    if (!roleView) {
      return <EmptyState icon={FolderX} title={t("mail.folderGone.title")} body={t("mail.folderGone.body")} />;
    }
    const role = t(`roles.${roleView}`);
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

  return <MailboxPanes key={mailbox.id} account={account} mailbox={mailbox} folderName={label(mailbox)} />;
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
  const dragRegion = useWindowDragRegion();
  const { accounts, account } = useSelectedAccount();
  const mailboxes = useMailboxes(view.kind === "mailbox" ? view.accountId : null);
  const label = useMailboxLabel();

  const folder = view.kind === "mailbox" ? mailboxes.data?.find((m) => m.id === view.mailboxId) : undefined;
  const title = view.kind === "mail" ? t(`nav.${view.mailView}`) : folder ? label(folder) : t("nav.folders");

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
    body = (
      <EmptyState
        icon={Inbox}
        title={t("mail.noAccount.title")}
        body={t("mail.noAccount.body")}
        action={{
          label: t("mail.noAccount.action"),
          onClick: () => {
            showAccountsPage({ kind: "add" });
          },
        }}
      />
    );
  } else {
    body = (
      <>
        <AccountErrorBanner account={account} />
        <MailBody view={view} account={account} />
      </>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ViewHeader title={title} subtitle={account && <AccountLine account={account} />} dragRegion={dragRegion}>
        <div className="flex min-w-0 items-center gap-3">
          {account && accounts.data && <SyncStatus accounts={accounts.data} account={account} />}
          <div className="flex shrink-0 items-center gap-1">
            <IconButton icon={Search} label={t("mail.search")} disabled />
            <IconButton icon={SquarePen} label={t("mail.compose")} disabled />
          </div>
        </div>
      </ViewHeader>
      {body}
    </div>
  );
}
