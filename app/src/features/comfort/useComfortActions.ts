import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { blockedSendersQueryKey } from "../blocked/queries";
import { refreshBlockedHidden } from "../reading/attachmentQueries";
import {
  comfortIpc,
  parityIpc,
  toCoreError,
  type BlockedSender,
  type Account,
  type ConversationIntent,
  type MessageListItem,
  type SnoozeChoice,
  type TaskMark,
} from "../../services/ipc";
import { useNoticeStore, type ComfortNotice } from "../../stores/notices";
import type { ActionHost } from "../actions/registry";

type ComfortHost = Pick<ActionHost, "setPinned" | "openSnooze" | "unsnooze" | "setTask" | "setReadingList" | "blockSenders">;

/**
 * Decision 0039: the messages an action on conversation rows touches (the core's answer to
 * `conversationTargets`); absent: the rows are messages and act on themselves.
 */
export type ExpandRows = (items: readonly MessageListItem[], intent: ConversationIntent) => Promise<readonly MessageListItem[]>;

/** Stage 16: the distinct sender addresses of messages (drafts are the user's own). */
export function senderAddresses(items: readonly MessageListItem[]): string[] {
  const seen = new Set<string>();
  for (const item of items) {
    const address = item.from?.email.trim().toLowerCase();
    if (address && !item.draft) seen.add(address);
  }
  return [...seen];
}

type TaskNotice = "taskAdded" | "taskRemoved" | "taskDone" | "taskReopened";

function taskNotice(mark: TaskMark | null, items: readonly MessageListItem[]): TaskNotice {
  if (mark === null) return "taskRemoved";
  if (mark.done) return "taskDone";
  return items.some((item) => item.task?.done) ? "taskReopened" : "taskAdded";
}

/**
 * Pins, snoozes and tasks of one view (stage 9). They are local: they work on read-only accounts
 * too and never move a message on the server; where writes are on, the core adds the `$Snoozed`
 * and `$Task` keywords through the outbox. After each change every list of the account reloads
 * (the marks show in folder lists as well); `inboxChanged` from the core does the same.
 */
export function useComfortActions(account: Account, expandRows?: ExpandRows) {
  const queryClient = useQueryClient();
  const push = useNoticeStore((state) => state.push);
  /**
   * The messages the snooze menu is open for, and what they touch: their conversations (rows),
   * or themselves (undefined: the reading pane's message, child rows of stage 17).
   */
  const [snoozing, setSnoozing] = useState<{ items: readonly MessageListItem[]; expand: ExpandRows | undefined } | null>(null);

  const reload = () => {
    void queryClient.invalidateQueries({ queryKey: ["messages", account.id] });
  };

  const run = async (
    what: Extract<ComfortNotice, { type: "failed" }>["what"],
    start: () => Promise<ComfortNotice | null>,
  ): Promise<boolean> => {
    try {
      const notice = await start();
      if (notice) push({ kind: "comfort", notice });
      return true;
    } catch (e) {
      push({ kind: "comfort", notice: { type: "failed", what, error: toCoreError(e) } });
      return false;
    } finally {
      reload();
    }
  };

  /**
   * The marks for `expand`: rows act on what the core says their conversations need (decision
   * 0039), the reading pane's one message on itself.
   */
  const makeHost = (expand: ExpandRows | undefined): ComfortHost => {
    const targets = async (items: readonly MessageListItem[], intent: ConversationIntent) => (expand ? expand(items, intent) : items);
    return {
      setPinned: (items, pinned) => {
        if (items.length === 0) return;
        void run("pin", async () => {
          const touched = await targets(items, { kind: "pin", pinned });
          if (touched.length === 0) return null;
          await comfortIpc.setPinned(
            touched.map((item) => item.id),
            pinned,
          );
          return { type: pinned ? "pinned" : "unpinned", count: items.length };
        });
      },
      openSnooze: (items) => {
        if (items.length === 0) return;
        setSnoozing({ items, expand });
      },
      unsnooze: (items) => {
        if (items.length === 0) return;
        void run("unsnooze", async () => {
          const touched = await targets(items, { kind: "unsnooze" });
          if (touched.length === 0) return null;
          await comfortIpc.unsnoozeMessages(touched.map((item) => item.id));
          return { type: "unsnoozed", count: items.length };
        });
      },
      setTask: (items, mark) => {
        if (items.length === 0) return;
        void run("task", async () => {
          const touched = await targets(items, { kind: "task", clear: mark === null });
          if (touched.length === 0) return null;
          await comfortIpc.setTask(
            touched.map((item) => item.id),
            mark,
          );
          return { type: taskNotice(mark, items), count: items.length };
        });
      },
      // Stage 16: local only, never a keyword on the server (decision 0033).
      setReadingList: (items, on) => {
        if (items.length === 0) return;
        void run("readingList", async () => {
          const touched = await targets(items, { kind: "readingList", on });
          if (touched.length === 0) return null;
          await parityIpc.setReadingList(
            touched.map((item) => item.id),
            on,
          );
          return { type: on ? "readingListAdded" : "readingListRemoved", count: items.length };
        });
      },
      // The senders of the rows' own messages (a conversation's row stands for its newest).
      blockSenders: (items) => {
        const addresses = senderAddresses(items);
        if (addresses.length === 0) return;
        void run("block", async () => {
          let list: BlockedSender[] = [];
          for (const address of addresses) list = await parityIpc.blockSender(address);
          queryClient.setQueryData(blockedSendersQueryKey, list);
          // Stage 16 (decision 0036): the inbox's note counts the mail that is hidden now.
          refreshBlockedHidden(queryClient);
          return { type: "sendersBlocked", addresses };
        });
      },
    };
  };

  /** The choice of the snooze menu for the messages it was opened for. */
  const snooze = async (choice: SnoozeChoice): Promise<boolean> => {
    const items = snoozing?.items ?? [];
    const expand = snoozing?.expand;
    setSnoozing(null);
    if (items.length === 0) return false;
    return run("snooze", async () => {
      // Decision 0039: a conversation is snoozed with its messages in the listed folder.
      const touched = expand ? await expand(items, { kind: "snooze" }) : items;
      if (touched.length === 0) return null;
      const messageIds = touched.map((item) => item.id);
      const result = await comfortIpc.snoozeMessages(messageIds, choice);
      return { type: "snoozed", count: items.length, until: result.until, messageIds };
    });
  };

  return {
    /** For list rows: conversations act as a whole while the conversation view is on. */
    host: makeHost(expandRows),
    /** For the one message open in the reading pane. */
    exactHost: makeHost(undefined),
    /** Stage 17: for what `expand` says (rows with child rows among them act per row). */
    hostWith: makeHost,
    snoozeItems: snoozing?.items ?? null,
    snooze,
    closeSnooze: () => {
      setSnoozing(null);
    },
  };
}

export type ComfortActions = ReturnType<typeof useComfortActions>;
