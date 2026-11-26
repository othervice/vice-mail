import { useQueryClient, type QueryKey } from "@tanstack/react-query";
import { useState, type RefObject } from "react";
import {
  conversationIpc,
  PARITY_LIMITS,
  parityIpc,
  toCoreError,
  type Account,
  type BatchId,
  type Mailbox,
  type MessageId,
  type MessageListItem,
} from "../../services/ipc";
import { roleFolder, useDragStore, type DropMode, type DropTarget } from "../../stores/drag";
import { useNoticeStore } from "../../stores/notices";
import { accountsQueryKey } from "../accounts/queries";
import { mailboxesQueryKey } from "../mailboxes/queries";
import type { MessageListHandle } from "../messages/MessageList";
import { messagesQueryKey } from "../messages/queries";
import { useFolderName } from "./folders";

interface DropRowsOptions {
  list: RefObject<MessageListHandle | null>;
  /** Decision 0039: the list's rows are conversations (a row stands for its conversation here). */
  conversations: boolean;
  /** The query of a list that mixes folders or accounts, reloaded after the drop. */
  listKey?: QueryKey | undefined;
}

function chunks<T>(items: readonly T[], size: number): T[][] {
  const parts: T[][] = [];
  for (let i = 0; i < items.length; i += size) parts.push(items.slice(i, i + size));
  return parts;
}

/** The rows of one drop that go to one folder. */
interface Group {
  mailbox: Mailbox;
  items: MessageListItem[];
}

/**
 * Stage 22 (decision 0081): dragging rows out of any list (a folder's, a view's, the unified
 * inbox, search results) and what the drop does. Rows may come from several folders and accounts:
 * the core moves those of the target's account at once and transfers those of other accounts
 * (copy first, the source removed once the copy arrived); rows already there stay. Dropping moves,
 * Option (macOS) or Ctrl (Windows) copies; the drop is the answer, so nothing asks first (the menu's
 * "Move to …" still asks before crossing accounts, decision 0033). Rows that leave their list at
 * once (a move within the account) are hidden and the cursor moves on, as for every action; one
 * notice for the whole drop, with "Undo" for the moves within an account.
 */
export function useDropRows({ list, conversations, listKey }: DropRowsOptions) {
  const queryClient = useQueryClient();
  const push = useNoticeStore((state) => state.push);
  const remember = useNoticeStore((state) => state.remember);
  const start = useDragStore((state) => state.start);
  const end = useDragStore((state) => state.end);
  const folderName = useFolderName();
  const [hiddenIds, setHiddenIds] = useState<ReadonlySet<MessageId>>(new Set());

  const hide = (ids: readonly MessageId[], hidden: boolean) => {
    if (ids.length === 0) return;
    setHiddenIds((current) => {
      const next = new Set(current);
      for (const id of ids) {
        if (hidden) next.add(id);
        else next.delete(id);
      }
      return next;
    });
  };

  /** The folder each row goes to: the target, or (rail) the role's folder of the row's account. */
  const groupsOf = (items: readonly MessageListItem[], target: DropTarget): Group[] => {
    if (target.kind === "folder") return [{ mailbox: target.mailbox, items: [...items] }];
    const groups = new Map<number, Group>();
    const accounts = queryClient.getQueryData<Account[]>(accountsQueryKey) ?? [];
    const context = {
      accounts,
      mailboxesOf: (accountId: string) => queryClient.getQueryData<Mailbox[]>(mailboxesQueryKey(accountId)) ?? [],
    };
    for (const item of items) {
      const mailbox = roleFolder(context, item.accountId, target.role);
      if (!mailbox) continue;
      const group = groups.get(mailbox.id) ?? { mailbox, items: [] };
      group.items.push(item);
      groups.set(mailbox.id, group);
    }
    return [...groups.values()];
  };

  /** The messages the rows stand for (decision 0039: a conversation row its conversation here). */
  const messagesOf = async (group: Group, childIds: ReadonlySet<MessageId>): Promise<readonly MessageListItem[]> => {
    const own = group.items.filter((item) => !conversations || childIds.has(item.id));
    const rows = group.items.filter((item) => !own.includes(item));
    if (rows.length === 0) return own;
    const expanded = await conversationIpc.targets(
      rows.map((item) => ({ mailboxId: item.mailboxId, messageId: item.id })),
      { kind: "action", action: { type: "move", target: group.mailbox.id } },
    );
    const seen = new Set<string>();
    return [...own, ...expanded].filter((item) => {
      const key = `${String(item.mailboxId)}:${String(item.id)}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  };

  const run = async (items: readonly MessageListItem[], childIds: ReadonlySet<MessageId>, target: DropTarget, mode: DropMode) => {
    const groups = groupsOf(items, target);
    if (groups.length === 0) return;
    const accounts = queryClient.getQueryData<Account[]>(accountsQueryKey) ?? [];
    // Rows that leave the list at once: moved within their own account, not acting as a child row.
    const leaving =
      mode === "move"
        ? groups.flatMap((group) =>
            group.items
              .filter((item) => item.accountId === group.mailbox.accountId && item.mailboxId !== group.mailbox.id && !childIds.has(item.id))
              .map((item) => item.id),
          )
        : [];
    if (leaving.length > 0) {
      list.current?.advancePast(leaving);
      hide(leaving, true);
      list.current?.focus();
    }
    const batches: BatchId[] = [];
    let moved = 0;
    let transferred = 0;
    let skipped = 0;
    let crossing = false;
    let failure: ReturnType<typeof toCoreError> | null = null;
    const touched = new Map<string, readonly [string, number]>();
    const touch = (accountId: string, mailboxId: number) => touched.set(`${accountId}:${String(mailboxId)}`, [accountId, mailboxId] as const);
    for (const group of groups) {
      touch(group.mailbox.accountId, group.mailbox.id);
      let messages: readonly MessageListItem[];
      try {
        messages = await messagesOf(group, childIds);
      } catch (e) {
        failure = toCoreError(e);
        break;
      }
      for (const item of messages) touch(item.accountId, item.mailboxId);
      crossing ||= messages.some((item) => item.accountId !== group.mailbox.accountId);
      for (const part of chunks(messages, PARITY_LIMITS.dropMessages)) {
        try {
          const refs = part.map((item) => ({ mailboxId: item.mailboxId, messageId: item.id }));
          const result = await parityIpc.dropMessages(refs, group.mailbox.id, mode, crypto.randomUUID());
          batches.push(...result.batches);
          moved += result.moved;
          transferred += result.transferred;
          skipped += result.skipped;
        } catch (e) {
          failure = toCoreError(e);
          break;
        }
      }
      if (failure) break;
    }
    remember(batches);
    const first = groups[0];
    if (first && (moved > 0 || transferred > 0 || (skipped > 0 && !failure))) {
      const owner = accounts.find((account) => account.id === first.mailbox.accountId);
      push({
        kind: "dropped",
        mode,
        moved,
        transferred,
        skipped,
        folder: folderName(first.mailbox),
        account: owner ? owner.displayName || owner.email : "",
        // The moves within an account undo like any move; the transfers only while nothing of
        // them reached the server (`undoEach` says what it could not take back).
        undo: mode === "move" && moved > 0 ? { accountId: first.mailbox.accountId, batches, partial: true } : null,
      });
    }
    if (failure) {
      switch (failure.code) {
        case "read_only": {
          // Turned off meanwhile (another window): the account list is out of date.
          void queryClient.invalidateQueries({ queryKey: accountsQueryKey });
          const sourceIds = new Set(items.map((item) => item.accountId));
          const refusing =
            accounts.find((account) => groups.some((group) => group.mailbox.accountId === account.id) && !account.writesEnabled) ??
            accounts.find((account) => sourceIds.has(account.id) && !account.writesEnabled) ??
            accounts.find((account) => account.id === first?.mailbox.accountId);
          if (refusing) push({ kind: "readOnly", accountId: refusing.id });
          break;
        }
        case "not_found":
          push({ kind: "outdated" });
          break;
        default:
          // Stage 16: the one refusal of a transfer the user can act on (no Message-ID).
          push({ kind: "error", context: crossing || mode === "copy" ? "transfer" : "drop", error: failure });
      }
    }
    // The core changed its local state before it answered: the reloaded lists no longer have the
    // moved rows (or have them back after a failure).
    await Promise.all([
      ...[...touched.values()].map(([accountId, mailboxId]) => queryClient.invalidateQueries({ queryKey: messagesQueryKey(accountId, mailboxId) })),
      ...(listKey ? [queryClient.invalidateQueries({ queryKey: listKey })] : []),
    ]);
    hide(leaving, false);
  };

  return {
    /** Rows a drop moved out of the list, hidden until the list reloaded without them. */
    hiddenIds,
    /** The list's `onDragRows`: a drag of these rows starts. */
    onDragRows: (items: readonly MessageListItem[], childIds: ReadonlySet<MessageId>) => {
      start({
        items,
        count: items.length,
        drop: (target, mode) => {
          void run(items, childIds, target, mode);
        },
      });
    },
    onDragEnd: end,
  };
}

/** Both sets of hidden rows of a list (its actions' and its drops'). */
export function unionIds(a: ReadonlySet<MessageId>, b: ReadonlySet<MessageId>): ReadonlySet<MessageId> {
  if (b.size === 0) return a;
  if (a.size === 0) return b;
  return new Set([...a, ...b]);
}
