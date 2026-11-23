import type { RefObject } from "react";
import { useTranslation } from "react-i18next";
import type { Account, Mailbox, MessageListItem } from "../../services/ipc";
import { ActionToolbar } from "../actions/ActionButtons";
import { MissingMailboxPrompt } from "../actions/MissingMailboxPrompt";
import { TOOLBAR_ACTIONS, type ActionId } from "../actions/registry";
import type { ShortcutPlatform } from "../actions/shortcuts";
import { useActionController } from "../actions/useActionController";
import { useActionShortcuts } from "../actions/useActionShortcuts";
import { useMarkReadOnOpen } from "../actions/useMarkReadOnOpen";
import { useMailboxes } from "../mailboxes/queries";
import type { MessageListHandle } from "../messages/MessageList";
import { ReadingPane, type FocusRequest } from "../reading/ReadingPane";

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
}

/** The open row with the actions of its own account and folder (one action controller each). */
function ReadingWithActions({ account, mailbox, item, focusRequest, platform, list }: ReadingProps & { mailbox: Mailbox }) {
  const { t } = useTranslation();
  const mailboxes = useMailboxes(account.id).data ?? [];
  const controller = useActionController({ account, mailbox, list });
  // Like the folder view: mark as read on open, and the shortcuts of the toolbar's actions.
  useMarkReadOnOpen(account, item, controller);
  useActionShortcuts({
    ctx: { account, mailbox, items: [item], canUndo: controller.canUndo },
    host: controller.host,
    platform,
    allowed: SHORTCUT_ACTIONS,
    blocked: controller.missing !== null || controller.overlay !== null,
    inList: (node) => list.current?.contains(node) ?? false,
  });
  const focusList = () => {
    list.current?.focus();
  };
  const closePane = () => {
    list.current?.clearSelection();
  };
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
      <ReadingPane
        account={account}
        messageId={item.id}
        focusRequest={focusRequest}
        toolbar={toolbar}
        onEscape={focusList}
        onClose={closePane}
      />
    </div>
  );
}

/**
 * The open row of a list that mixes folders or accounts (unified inbox, search results): the
 * reading pane with the actions of the row's own account and folder once that folder is known.
 */
export function MixedReading(props: ReadingProps) {
  const mailbox = useMailboxes(props.account.id).data?.find((m) => m.id === props.item.mailboxId);
  if (mailbox) return <ReadingWithActions key={`${String(mailbox.id)}-${String(props.item.id)}`} {...props} mailbox={mailbox} />;
  const focusList = () => {
    props.list.current?.focus();
  };
  const closePane = () => {
    props.list.current?.clearSelection();
  };
  return (
    <ReadingPane
      key={props.item.id}
      account={props.account}
      messageId={props.item.id}
      focusRequest={props.focusRequest}
      onEscape={focusList}
      onClose={closePane}
    />
  );
}
