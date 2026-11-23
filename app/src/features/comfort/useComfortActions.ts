import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { blockedSendersQueryKey } from "../blocked/queries";
import {
  comfortIpc,
  parityIpc,
  toCoreError,
  type BlockedSender,
  type Account,
  type MessageListItem,
  type SnoozeChoice,
  type TaskMark,
} from "../../services/ipc";
import { useNoticeStore, type ComfortNotice } from "../../stores/notices";
import type { ActionHost } from "../actions/registry";

type ComfortHost = Pick<ActionHost, "setPinned" | "openSnooze" | "unsnooze" | "setTask" | "setReadingList" | "blockSenders">;

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
export function useComfortActions(account: Account) {
  const queryClient = useQueryClient();
  const push = useNoticeStore((state) => state.push);
  const [snoozeItems, setSnoozeItems] = useState<readonly MessageListItem[] | null>(null);

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

  const host: ComfortHost = {
    setPinned: (items, pinned) => {
      if (items.length === 0) return;
      void run("pin", async () => {
        await comfortIpc.setPinned(
          items.map((item) => item.id),
          pinned,
        );
        return { type: pinned ? "pinned" : "unpinned", count: items.length };
      });
    },
    openSnooze: (items) => {
      if (items.length > 0) setSnoozeItems(items);
    },
    unsnooze: (items) => {
      if (items.length === 0) return;
      void run("unsnooze", async () => {
        await comfortIpc.unsnoozeMessages(items.map((item) => item.id));
        return { type: "unsnoozed", count: items.length };
      });
    },
    setTask: (items, mark) => {
      if (items.length === 0) return;
      void run("task", async () => {
        await comfortIpc.setTask(
          items.map((item) => item.id),
          mark,
        );
        return { type: taskNotice(mark, items), count: items.length };
      });
    },
    // Stage 16: local only, never a keyword on the server (decision 0033).
    setReadingList: (items, on) => {
      if (items.length === 0) return;
      void run("readingList", async () => {
        await parityIpc.setReadingList(
          items.map((item) => item.id),
          on,
        );
        return { type: on ? "readingListAdded" : "readingListRemoved", count: items.length };
      });
    },
    blockSenders: (items) => {
      const addresses = senderAddresses(items);
      if (addresses.length === 0) return;
      void run("block", async () => {
        let list: BlockedSender[] = [];
        for (const address of addresses) list = await parityIpc.blockSender(address);
        queryClient.setQueryData(blockedSendersQueryKey, list);
        return { type: "sendersBlocked", addresses };
      });
    },
  };

  /** The choice of the snooze menu for the messages it was opened for. */
  const snooze = async (choice: SnoozeChoice): Promise<boolean> => {
    const items = snoozeItems ?? [];
    setSnoozeItems(null);
    if (items.length === 0) return false;
    const messageIds = items.map((item) => item.id);
    return run("snooze", async () => {
      const result = await comfortIpc.snoozeMessages(messageIds, choice);
      return { type: "snoozed", count: items.length, until: result.until, messageIds };
    });
  };

  return {
    host,
    snoozeItems,
    snooze,
    closeSnooze: () => {
      setSnoozeItems(null);
    },
  };
}

export type ComfortActions = ReturnType<typeof useComfortActions>;
