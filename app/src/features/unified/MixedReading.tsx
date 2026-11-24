import { useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import type { Account, Mailbox, MessageId, MessageListItem } from "../../services/ipc";
import { ActionToolbar } from "../actions/ActionButtons";
import { MissingMailboxPrompt } from "../actions/MissingMailboxPrompt";
import { TOOLBAR_ACTIONS, type ActionId } from "../actions/registry";
import type { ShortcutPlatform } from "../actions/shortcuts";
import { useActionController } from "../actions/useActionController";
import { useActionShortcuts } from "../actions/useActionShortcuts";
import { useMarkReadOnOpen } from "../actions/useMarkReadOnOpen";
import { useMailboxes } from "../mailboxes/queries";
import type { ReadingLayout } from "../layout/useReadingLayout";
import type { MessageListHandle } from "../messages/MessageList";
import { ReadingPane, type FocusRequest } from "../reading/ReadingPane";
import { ThreadPane } from "../threads/ThreadPane";
import { conversationIdent } from "../threads/tree";

/**
 * Actions of the reading pane in a mixed list: those that need no picker or dialog of the folder
 * view (moving, tags, snoozing and deleting for good open one there; the folder view has them).
 */
const OVERLAY_ACTIONS: ReadonlySet<ActionId> = new Set(["deletePermanently", "move", "tags", "snooze"]);
export const UNIFIED_TOOLBAR: readonly ActionId[] = TOOLBAR_ACTIONS.filter((id) => !OVERLAY_ACTIONS.has(id));
const SHORTCUT_ACTIONS: ReadonlySet<ActionId> = new Set([...UNIFIED_TOOLBAR, "undo"]);

interface ReadingProps {
  account: Account;
  item: MessageListItem;
  focusRequest: FocusRequest | null;
  platform: ShortcutPlatform;
  list: RefObject<MessageListHandle | null>;
  /** Stage 8: where the pane sits; Escape and closing go through it (in place of the list). */
  layout?: Pick<ReadingLayout, "backToList" | "close">;
  /**
   * Decision 0039: the row is a conversation: the list's shortcuts act on the conversation, the
   * toolbar on the open message. Stage 17 (decision 0049): the pane shows one message of it (the
   * oldest unread, else the newest); a message chosen as a child row is no conversation row.
   */
  conversation?: boolean;
  /** Stage 17: the conversation row's identity in the list (its thread). */
  ident?: string;
  /** Stage 17: a source of an AI answer among the conversation's messages: show it in the list. */
  onShowMessage?: (messageId: MessageId) => void;
}

/** Escape and the close button of the pane, by the layout of the list it belongs to. */
function paneExits(list: ReadingProps["list"], layout: ReadingProps["layout"]) {
  const focusList = () => {
    list.current?.focus();
  };
  const clearSelection = () => {
    list.current?.clearSelection();
  };
  return {
    focusList,
    onEscape: () => {
      if (layout) layout.backToList(focusList);
      else focusList();
    },
    onClose: () => {
      if (layout) layout.close(clearSelection);
      else clearSelection();
    },
  };
}

/** The open row with the actions of its own account and folder (one action controller each). */
function ReadingWithActions({
  account,
  mailbox,
  item,
  focusRequest,
  platform,
  list,
  layout,
  conversation = false,
  ident,
  onShowMessage,
}: ReadingProps & { mailbox: Mailbox }) {
  const { t } = useTranslation();
  const mailboxes = useMailboxes(account.id).data ?? [];
  const controller = useActionController({ account, mailbox, list, conversations: conversation });
  // Stage 17: a conversation row's pane names the message it shows once it knows it.
  const [shown, setShown] = useState<MessageListItem | null>(null);
  // Like the folder view: mark as read on open (the message shown), and the shortcuts of the
  // toolbar's actions.
  useMarkReadOnOpen(account, conversation ? shown : item, controller, { exact: true });
  useActionShortcuts({
    ctx: { account, mailbox, items: [item], canUndo: controller.canUndo },
    host: controller.host,
    platform,
    allowed: SHORTCUT_ACTIONS,
    blocked: controller.missing !== null || controller.overlay !== null,
    inList: (node) => list.current?.contains(node) ?? false,
  });
  const { focusList, onEscape, onClose } = paneExits(list, layout);
  const toolbar = (
    <ActionToolbar
      ids={UNIFIED_TOOLBAR}
      ctx={{ account, mailbox, items: [item], canUndo: controller.canUndo }}
      host={controller.host}
      platform={platform}
      label={t("actions.toolbar")}
    />
  );
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      {controller.missing && (
        <MissingMailboxPrompt
          key={`${controller.missing.role}-${String(controller.missing.items.length)}`}
          account={account}
          mailboxes={mailboxes}
          missing={controller.missing}
          onChoose={controller.chooseFolderForRole}
          onCreate={controller.createFolderForRole}
          onCancel={() => {
            controller.cancelMissing();
            focusList();
          }}
        />
      )}
      {conversation ? (
        <ThreadPane
          account={account}
          item={item}
          ident={ident ?? conversationIdent(item, undefined)}
          focusRequest={focusRequest}
          onShown={setShown}
          {...(onShowMessage && { onShowMessage })}
          // The open message's own actions, where it lies in the row's folder.
          toolbarFor={(message) =>
            message.mailboxId === mailbox.id && (
              <ActionToolbar
                ids={UNIFIED_TOOLBAR}
                ctx={{ account, mailbox, items: [message], canUndo: controller.canUndo }}
                host={controller.messageHost}
                platform={platform}
                label={t("actions.toolbar")}
              />
            )
          }
          onEscape={onEscape}
          onClose={onClose}
        />
      ) : (
        <ReadingPane
          account={account}
          messageId={item.id}
          focusRequest={focusRequest}
          toolbar={toolbar}
          onEscape={onEscape}
          onClose={onClose}
        />
      )}
    </div>
  );
}

/**
 * The open row of a list that mixes folders or accounts (unified inbox, search results): the
 * reading pane with the actions of the row's own account and folder once that folder is known.
 */
export function MixedReading(props: ReadingProps) {
  const mailbox = useMailboxes(props.account.id).data?.find((m) => m.id === props.item.mailboxId);
  // A conversation row keeps its pane while a newer message becomes the row's (its identity).
  const key = props.conversation && props.ident ? props.ident : String(props.item.id);
  if (mailbox) return <ReadingWithActions key={`${String(mailbox.id)}-${key}`} {...props} mailbox={mailbox} />;
  const { onEscape, onClose } = paneExits(props.list, props.layout);
  if (props.conversation) {
    return (
      <ThreadPane
        key={key}
        account={props.account}
        item={props.item}
        ident={props.ident ?? key}
        focusRequest={props.focusRequest}
        {...(props.onShowMessage && { onShowMessage: props.onShowMessage })}
        onEscape={onEscape}
        onClose={onClose}
      />
    );
  }
  return (
    <ReadingPane
      key={props.item.id}
      account={props.account}
      messageId={props.item.id}
      focusRequest={props.focusRequest}
      onEscape={onEscape}
      onClose={onClose}
    />
  );
}
