import { FileX } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { toCoreError, uiIpc, type Account, type AccountId, type CoreError, type Mailbox, type MessageId, type MessageListItem } from "../../services/ipc";
import { useComposerStore } from "../../stores/composer";
import { useAccounts } from "../accounts/queries";
import { ActionToolbar } from "../actions/ActionButtons";
import { MissingMailboxPrompt } from "../actions/MissingMailboxPrompt";
import { Notices } from "../actions/Notices";
import { MovePicker, TagEditor } from "../actions/Pickers";
import { TOOLBAR_ACTIONS, type ActionContext, type ActionId } from "../actions/registry";
import { shortcutPlatform } from "../actions/shortcuts";
import { useActionController } from "../actions/useActionController";
import { useActionShortcuts } from "../actions/useActionShortcuts";
import { useMarkReadOnOpen } from "../actions/useMarkReadOnOpen";
import { useAppInfo } from "../app-info/queries";
import { SnoozePicker } from "../comfort/SnoozePicker";
import { ComposerHost } from "../composer/Composer";
import { SendNotices } from "../composer/SendNotices";
import { RedirectHost } from "../redirect/RedirectDialog";
import { useMailboxes } from "../mailboxes/queries";
import { useMailboxLabel } from "../mail/MailView";
import type { MessageListHandle } from "../messages/MessageList";
import { ReadingPane } from "../reading/ReadingPane";
import { useThread } from "../threads/queries";

/** The toolbar of the message window: everything of the reading pane but opening it again. */
export const MESSAGE_WINDOW_ACTIONS: readonly ActionId[] = TOOLBAR_ACTIONS.filter((id) => id !== "openInWindow");
const ALLOWED: ReadonlySet<ActionId> = new Set(MESSAGE_WINDOW_ACTIONS);

/** Asks the shell to close this window; when it may not, the window says how to close it. */
export function useCloseThisWindow(): { close: () => void; error: CoreError | null } {
  const [error, setError] = useState<CoreError | null>(null);
  return {
    close: () => {
      setError(null);
      uiIpc.closeThisWindow().catch((e: unknown) => {
        setError(toCoreError(e));
      });
    },
    error,
  };
}

interface PaneProps {
  account: Account;
  mailbox: Mailbox;
  item: MessageListItem;
  onClose: () => void;
}

/** The reading pane for the one message with the same toolbar, shortcuts and pickers as in the list. */
function MessageWindowPane({ account, mailbox, item, onClose }: PaneProps) {
  const { t } = useTranslation();
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const mailboxes = useMailboxes(account.id).data;
  const label = useMailboxLabel();
  // There is no list in this window: moving on after an action has nothing to move to.
  const list = useRef<MessageListHandle>(null);
  const controller = useActionController({ account, mailbox, list });
  useMarkReadOnOpen(account, item, controller);
  const ctx: ActionContext = { account, mailbox, items: [item], canUndo: controller.canUndo };
  const pickerOpen = Boolean(controller.overlay || controller.comfort.snoozeItems || controller.missing);
  useActionShortcuts({ ctx, host: controller.host, platform, allowed: ALLOWED, blocked: pickerOpen, inList: () => false });
  const { overlay } = controller;
  const article = () => {
    document.querySelector<HTMLElement>("[data-print-root]")?.focus();
  };

  return (
    <>
      {controller.missing && (
        <div className="px-6 pt-4">
          <MissingMailboxPrompt
            key={`${controller.missing.role}-${String(controller.missing.items.length)}`}
            account={account}
            mailboxes={mailboxes ?? []}
            missing={controller.missing}
            onChoose={controller.chooseFolderForRole}
            onCreate={controller.createFolderForRole}
            onCancel={() => {
              controller.cancelMissing();
              article();
            }}
          />
        </div>
      )}
      <ReadingPane
        account={account}
        messageId={item.id}
        focusRequest={null}
        toolbar={<ActionToolbar ids={MESSAGE_WINDOW_ACTIONS} ctx={ctx} host={controller.host} platform={platform} label={t("actions.toolbar")} />}
        onEscape={article}
        onClose={onClose}
        draft={item.draft}
        // The AI panel and "open in a window" belong to the main window.
        inWindow
      />
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
          focusAfterConfirm={article}
          data-testid="confirm-delete"
        />
      )}
      {controller.comfort.snoozeItems && (
        <SnoozePicker
          count={controller.comfort.snoozeItems.length}
          onChoose={(choice) => {
            void controller.comfort.snooze(choice);
          }}
          onClose={controller.comfort.closeSnooze}
          restoreFocus={article}
        />
      )}
      {overlay?.kind === "move" && (
        <MovePicker
          count={1}
          mailboxes={mailboxes ?? []}
          current={mailbox}
          onChoose={(target) => {
            controller.closeOverlay();
            void controller.apply({ type: "move", target: target.id }, [item], { detail: label(target) });
          }}
          onClose={controller.closeOverlay}
          restoreFocus={article}
        />
      )}
      {overlay?.kind === "tags" && (
        <TagEditor
          items={[item]}
          known={[item]}
          onAdd={(tag) => controller.apply({ type: "addKeyword", keyword: tag }, [item], { detail: tag, ownErrors: ["invalid_keyword"] })}
          onRemove={(tag) => controller.apply({ type: "removeKeyword", keyword: tag }, [item], { detail: tag })}
          onClose={controller.closeOverlay}
          restoreFocus={article}
        />
      )}
    </>
  );
}

interface MessageWindowProps {
  accountId: AccountId;
  messageId: MessageId;
  eventsError: CoreError | null;
}

/**
 * Stage 8 (decision 0025): the window of one message, opened with "Open in new window". No
 * rail and no list: the reading pane with its toolbar, the composer for replies above it, and
 * the notices. The message is looked up by its id (`getThread` names its folder and flags); if
 * it or its account is gone, the window says so.
 */
export function MessageWindow({ accountId, messageId, eventsError }: MessageWindowProps) {
  const { t } = useTranslation();
  const accounts = useAccounts();
  const account = accounts.data?.find((a) => a.id === accountId) ?? null;
  const thread = useThread(accountId, messageId, null);
  const mailboxes = useMailboxes(account ? accountId : null);
  const item = thread.data?.messages.find((entry) => entry.message.id === messageId)?.message ?? null;
  const mailbox = item ? (mailboxes.data?.find((m) => m.id === item.mailboxId) ?? null) : null;
  const closing = useCloseThisWindow();

  // Drafts of the last session are offered by the main window, not here.
  useEffect(() => {
    useComposerStore.getState().finishRestore();
  }, []);

  let content;
  if (accounts.isError) {
    content = <ErrorNotice context={t("windows.message.loadFailed")} error={accounts.error} onRetry={() => void accounts.refetch()} />;
  } else if (thread.isError && thread.error.code !== "not_found") {
    content = <ErrorNotice context={t("windows.message.loadFailed")} error={thread.error} onRetry={() => void thread.refetch()} />;
  } else if (mailboxes.isError) {
    content = <ErrorNotice context={t("windows.message.loadFailed")} error={mailboxes.error} onRetry={() => void mailboxes.refetch()} />;
  } else if ((accounts.data && !account) || (thread.isError && thread.error.code === "not_found") || (thread.data && !item) || (mailboxes.data && item && !mailbox)) {
    content = <EmptyState icon={FileX} title={t("windows.message.gone.title")} body={t("windows.message.gone.body")} />;
  } else if (!account || !item || !mailbox) {
    content = (
      <p role="status" className="p-6 text-13 text-secondary" data-testid="message-window-loading">
        {t("reading.loading")}
      </p>
    );
  } else {
    content = <MessageWindowPane account={account} mailbox={mailbox} item={item} onClose={closing.close} />;
  }

  return (
    <div className="flex h-full bg-canvas text-primary" data-window="message">
      <main tabIndex={-1} className="relative flex min-w-0 flex-1 flex-col bg-surface outline-none" aria-label={t("windows.message.region")}>
        {eventsError && (
          <div className="px-6 pt-4">
            <ErrorNotice context={t("errors.context.events")} error={eventsError} />
          </div>
        )}
        {closing.error && (
          <div className="px-6 pt-4">
            <ErrorNotice context={t("windows.closeFailed")} error={closing.error} data-testid="window-close-failed" />
          </div>
        )}
        {content}
        <ComposerHost />
      </main>
      <Notices />
      <SendNotices />
      <RedirectHost />
    </div>
  );
}
