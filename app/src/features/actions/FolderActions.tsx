import type { QueryKey } from "@tanstack/react-query";
import { useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { conversationIpc, toCoreError, type Account, type Mailbox, type MessageId, type MessageListItem } from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";
import { FollowUpPicker } from "../comfort/FollowUpPicker";
import { SnoozePicker } from "../comfort/SnoozePicker";
import type { MessageListHandle } from "../messages/MessageList";
import { useFolderName } from "../organize/folders";
import { TransferDialog, useOtherAccountFolders, useTransfer, type TransferRequest } from "../organize/transfer";
import { MissingMailboxPrompt } from "./MissingMailboxPrompt";
import { MovePicker, TagEditor } from "./Pickers";
import { useActionController, type Exactness } from "./useActionController";

interface FolderActionsOptions {
  account: Account;
  mailbox: Mailbox;
  list: RefObject<MessageListHandle | null>;
  /** Decision 0039: the list's rows are conversations. */
  conversations: boolean;
  /** Stage 21: the query of a list that mixes folders or accounts (reloaded after actions). */
  listKey?: QueryKey | undefined;
}

/**
 * The actions of one folder's messages, whichever surface starts them (quick actions, toolbar,
 * selection bar, shortcuts, drag and drop, stage 21's context menu): the action controller, and
 * moving or copying to a folder of this or another account (decision 0033: another account asks
 * first for a move; stage 21: a copy runs at once, the user chose it).
 */
export function useFolderActions({ account, mailbox, list, conversations, listKey }: FolderActionsOptions) {
  const controller = useActionController({ account, mailbox, list, conversations, listKey });
  const push = useNoticeStore((state) => state.push);
  const label = useFolderName();
  const otherFolders = useOtherAccountFolders(account.id);
  const transfer = useTransfer();
  const [transferRequest, setTransferRequest] = useState<TransferRequest | null>(null);

  /**
   * The messages `items` stand for in a move or copy to `target`: themselves while the rows are
   * messages; decision 0039: a conversation row its messages in this folder (`exact`: the one
   * message of the reading pane; stage 17: a set names child rows, which go alone).
   */
  const messagesFor = (target: Mailbox, items: readonly MessageListItem[], exact: Exactness, then: (messages: readonly MessageListItem[]) => void) => {
    const own = exact === true ? items : exact === false ? [] : items.filter((item) => exact.has(item.id));
    const rows = items.filter((item) => !own.includes(item));
    if (!conversations || rows.length === 0) {
      then(items);
      return;
    }
    conversationIpc
      .targets(
        rows.map((item) => ({ mailboxId: item.mailboxId, messageId: item.id })),
        { kind: "action", action: { type: "move", target: target.id } },
      )
      .then(
        (targets) => {
          then([...own, ...targets]);
        },
        (error: unknown) => {
          push({ kind: "error", context: "action", error: toCoreError(error) });
        },
      );
  };

  /** The account a folder belongs to: this one, or another one the user has. */
  const accountOf = (target: Mailbox): Account | undefined =>
    target.accountId === account.id ? account : otherFolders.find((entry) => entry.account.id === target.accountId)?.account;

  /** A folder of another account asks first (decision 0033); one of this account moves at once. */
  const moveOrAsk = (target: Mailbox, items: readonly MessageListItem[], exact: Exactness = false) => {
    if (target.accountId === account.id) {
      void controller.apply({ type: "move", target: target.id }, items, { detail: label(target), exact });
      return;
    }
    const targetAccount = accountOf(target);
    if (!targetAccount || items.length === 0) return;
    messagesFor(target, items, exact, (moved) => {
      if (moved.length > 0) setTransferRequest({ items: moved, source: { account, mailbox }, target: { account: targetAccount, mailbox: target } });
    });
  };

  /**
   * Stage 21 (decision 0064): a copy into a folder of this account or another one, through the
   * outbox like a transfer between accounts (`transferMessages`, mode copy). No question first:
   * nothing leaves its folder, and the user chose "Copy to".
   */
  const copyTo = (target: Mailbox, items: readonly MessageListItem[], exact: Exactness = false) => {
    const targetAccount = accountOf(target);
    if (!targetAccount || items.length === 0) return;
    messagesFor(target, items, exact, (copied) => {
      if (copied.length > 0) void transfer({ items: copied, source: { account, mailbox }, target: { account: targetAccount, mailbox: target } }, "copy");
    });
  };

  return { account, mailbox, controller, moveOrAsk, copyTo, otherFolders, transfer, transferRequest, setTransferRequest };
}

export type FolderActions = ReturnType<typeof useFolderActions>;

/** The inline question when an action found no folder for its role (archive, trash, junk). */
export function FolderMissingPrompt({ actions, mailboxes, focusList }: { actions: FolderActions; mailboxes: readonly Mailbox[]; focusList: () => void }) {
  const { controller, account } = actions;
  if (!controller.missing) return null;
  return (
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
  );
}

interface FolderActionOverlaysProps {
  actions: FolderActions;
  /** The folders of the actions' account. */
  mailboxes: readonly Mailbox[];
  /** The messages the list has loaded: the pickers act on their current state, by id. */
  loaded: readonly MessageListItem[];
  /** Where the focus goes when a picker or dialog closes. */
  focusList: () => void;
}

/**
 * The dialogs and pickers the actions open: deleting for good, snoozing, the reminder if no reply
 * comes, moving (also to another account, which asks first), and tags.
 */
export function FolderActionOverlays({ actions, mailboxes, loaded, focusList }: FolderActionOverlaysProps) {
  const { t } = useTranslation();
  const { controller, mailbox, transferRequest, setTransferRequest, transfer } = actions;
  const { overlay } = controller;
  const byId = new Map(loaded.map((item) => [item.id, item]));
  const fresh = (ids: readonly MessageId[]) => ids.flatMap((id) => byId.get(id) ?? []);
  return (
    <>
      {overlay?.kind === "confirmDelete" && (
        <ConfirmDialog
          title={t("actions.confirmDelete.title", { count: overlay.items.length })}
          body={t("actions.confirmDelete.body")}
          confirmLabel={t("actions.confirmDelete.confirm")}
          cancelLabel={t("actions.confirmDelete.cancel")}
          onConfirm={() => {
            const { items, exact } = overlay;
            controller.closeOverlay();
            void controller.apply({ type: "deletePermanently" }, items, { exact: exact ?? false });
          }}
          onCancel={controller.closeOverlay}
          focusAfterConfirm={focusList}
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
          restoreFocus={focusList}
        />
      )}
      {/* Stage 20 (decision 0059): "Remind me if no reply". */}
      {controller.comfort.followUpItems && (
        <FollowUpPicker
          count={controller.comfort.followUpItems.length}
          onChoose={(choice) => {
            void controller.comfort.followUp(choice);
          }}
          onClose={controller.comfort.closeFollowUp}
          restoreFocus={focusList}
        />
      )}
      {overlay?.kind === "move" && (
        <MovePicker
          count={overlay.ids.length}
          mailboxes={mailboxes}
          current={mailbox}
          otherAccounts={actions.otherFolders}
          onChoose={(target) => {
            const items = fresh(overlay.ids);
            controller.closeOverlay();
            actions.moveOrAsk(target, items, overlay.exact ?? false);
          }}
          onClose={controller.closeOverlay}
          restoreFocus={focusList}
        />
      )}
      {transferRequest && (
        <TransferDialog
          request={transferRequest}
          onChoose={(mode) => {
            const request = transferRequest;
            setTransferRequest(null);
            void transfer(request, mode);
          }}
          onCancel={() => {
            setTransferRequest(null);
          }}
          focusAfter={focusList}
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
              exact: overlay.exact ?? false,
            })
          }
          onRemove={(tag) =>
            controller.apply(
              { type: "removeKeyword", keyword: tag },
              fresh(overlay.ids).filter((item) => item.keywords.some((k) => k.toLowerCase() === tag.toLowerCase())),
              { detail: tag, exact: overlay.exact ?? false },
            )
          }
          onClose={controller.closeOverlay}
          restoreFocus={focusList}
        />
      )}
    </>
  );
}
