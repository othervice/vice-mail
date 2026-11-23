import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState, type RefObject } from "react";
import {
  conversationIpc,
  ipc,
  MAX_ACTION_MESSAGES,
  toCoreError,
  type Account,
  type BatchId,
  type ConversationIntent,
  type CoreError,
  type MailAction,
  type Mailbox,
  type MailboxRole,
  type MessageId,
  type MessageListItem,
} from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";
import { accountsQueryKey } from "../accounts/queries";
import { useComfortActions } from "../comfort/useComfortActions";
import { mailboxesQueryKey } from "../mailboxes/queries";
import type { MessageListHandle } from "../messages/MessageList";
import { messagesQueryKey } from "../messages/queries";
import { actionKind, leavesFolder, requiredRole, type ActionHost } from "./registry";
import { undoAction } from "./undo";

/**
 * Overlays an action opens: the one dialog (permanent delete) and the two pickers. `exact`: opened
 * for the one message of the reading pane, not for conversation rows (decision 0039).
 */
export type ActionOverlay =
  | { kind: "confirmDelete"; items: readonly MessageListItem[]; exact?: boolean }
  | { kind: "move"; ids: readonly MessageId[]; exact?: boolean }
  | { kind: "tags"; ids: readonly MessageId[]; exact?: boolean };

/** An action that found no folder for its role (`missing_mailbox`): the inline question. */
export interface MissingMailbox {
  role: MailboxRole;
  action: MailAction;
  items: readonly MessageListItem[];
  /** A folder is being created for it; the action runs once it exists with the role. */
  creating: string | null;
  /** Decision 0039: asked for the one message of the reading pane, not conversation rows. */
  exact?: boolean;
}

export interface ApplyOptions {
  /** No notice (marking as read on open). */
  silent?: boolean;
  /** Folder name or tag for the notice ("Moved to Projects"). */
  detail?: string;
  /** The caller shows these errors itself (the tag field shows `invalid_keyword`). */
  ownErrors?: readonly CoreError["code"][];
  /** What failed, for the error notice. */
  errorContext?: "action" | "markRead";
  /**
   * Decision 0039: the messages themselves, also while the rows are conversations (the one
   * message open in the reading pane, marking as read on open).
   */
  exact?: boolean;
}

export type ApplyOutcome = { ok: true } | { ok: false; error: CoreError | null };

interface ControllerOptions {
  account: Account;
  mailbox: Mailbox;
  list: RefObject<MessageListHandle | null>;
  /**
   * Decision 0039: the list's rows are conversations. An action on rows then acts on the messages
   * the core names for it (`conversationTargets`: the conversation in this folder, everywhere, or
   * its newest message), unless it is `exact`.
   */
  conversations?: boolean;
}

function chunks<T>(items: readonly T[], size: number): T[][] {
  const parts: T[][] = [];
  for (let i = 0; i < items.length; i += size) parts.push(items.slice(i, i + size));
  return parts;
}

/**
 * Resolves once the folder list of the account has a folder with `role` (a created folder
 * appears with `mailboxesChanged` when the server made it). Rejects when `signal` aborts.
 */
function waitForRole(queryClient: QueryClient, account: Account, role: MailboxRole, signal: AbortSignal): Promise<Mailbox> {
  return new Promise((resolve, reject) => {
    const key = mailboxesQueryKey(account.id);
    const find = () => queryClient.getQueryData<Mailbox[]>(key)?.find((m) => m.role === role);
    const found = find();
    if (found) {
      resolve(found);
      return;
    }
    const unsubscribe = queryClient.getQueryCache().subscribe(() => {
      const mailbox = find();
      if (!mailbox) return;
      unsubscribe();
      resolve(mailbox);
    });
    signal.addEventListener("abort", () => {
      unsubscribe();
      reject(new DOMException("cancelled", "AbortError"));
    });
  });
}

/**
 * Runs the mail actions of one folder view, whichever surface started them (quick action,
 * toolbar, shortcut, drag and drop): writes check, one `applyAction` per user action with a fresh
 * request id (split into parts of `MAX_ACTION_MESSAGES`), rows that leave the folder hidden at
 * once and the cursor moved to the next mail, the undo notice, and the answers to the core's
 * questions (which folder, delete for good?).
 */
export function useActionController({ account, mailbox, list, conversations = false }: ControllerOptions) {
  const queryClient = useQueryClient();
  const push = useNoticeStore((state) => state.push);
  const remember = useNoticeStore((state) => state.remember);
  const lastUndo = useNoticeStore((state) => state.lastUndo);
  const [hiddenIds, setHiddenIds] = useState<ReadonlySet<MessageId>>(new Set());
  const [overlay, setOverlay] = useState<ActionOverlay | null>(null);
  const [missing, setMissing] = useState<MissingMailbox | null>(null);
  const waiting = useRef<AbortController | null>(null);
  /** Decision 0039: what an action on conversation rows touches, as the core decides. */
  const expand = async (items: readonly MessageListItem[], intent: ConversationIntent): Promise<readonly MessageListItem[]> =>
    conversationIpc.targets(
      items.map((item) => ({ mailboxId: item.mailboxId, messageId: item.id })),
      intent,
    );
  // Stage 9: pins, snoozes and tasks (local marks, their own menu).
  const comfort = useComfortActions(account, conversations ? expand : undefined);

  // A folder creation still waited for belongs to this view only.
  useEffect(
    () => () => {
      waiting.current?.abort();
    },
    [],
  );

  const hide = (ids: readonly MessageId[], hidden: boolean) => {
    setHiddenIds((current) => {
      const next = new Set(current);
      for (const id of ids) {
        if (hidden) next.add(id);
        else next.delete(id);
      }
      return next;
    });
  };

  const handleError = (
    error: CoreError,
    action: MailAction,
    items: readonly MessageListItem[],
    context: "action" | "markRead",
    exact = false,
  ) => {
    switch (error.code) {
      case "missing_mailbox": {
        const role = requiredRole(action);
        if (role) {
          setMissing({ role, action, items, creating: null, exact });
          return;
        }
        break;
      }
      case "confirmation_required":
        setOverlay({ kind: "confirmDelete", items, exact });
        return;
      case "read_only":
        // Turned off meanwhile (another window): the account list is out of date.
        void queryClient.invalidateQueries({ queryKey: accountsQueryKey });
        push({ kind: "readOnly", accountId: account.id });
        return;
      case "not_found":
        void queryClient.invalidateQueries({ queryKey: messagesQueryKey(account.id, mailbox.id) });
        push({ kind: "outdated" });
        return;
      default:
        break;
    }
    push({ kind: "error", context, error });
  };

  const apply = async (
    action: MailAction,
    items: readonly MessageListItem[],
    options: ApplyOptions = {},
  ): Promise<ApplyOutcome> => {
    if (items.length === 0) return { ok: false, error: null };
    if (!account.writesEnabled) {
      if (!options.silent) push({ kind: "readOnly", accountId: account.id });
      return { ok: false, error: null };
    }
    const rowIds = items.map((item) => item.id);
    const leaves = leavesFolder(action, mailbox);
    if (leaves) {
      // The next mail takes the cursor before the rows go (DESIGN.md: focus on the next mail).
      list.current?.advancePast(rowIds);
      hide(rowIds, true);
      list.current?.focus();
    }
    // Decision 0039: conversation rows act on the messages the core names for the action.
    let targets: readonly MessageListItem[] = items;
    if (conversations && !options.exact) {
      try {
        targets = await expand(items, { kind: "action", action });
      } catch (e) {
        if (leaves) hide(rowIds, false);
        handleError(toCoreError(e), action, items, options.errorContext ?? "action", false);
        return { ok: false, error: toCoreError(e) };
      }
      if (targets.length === 0) {
        if (leaves) hide(rowIds, false);
        return { ok: false, error: null };
      }
    }
    const ids = [...new Set([...rowIds, ...targets.map((item) => item.id)])];
    if (leaves) hide(ids, true);
    const batches: BatchId[] = [];
    let done = 0;
    let recorded = false;
    let failure: CoreError | null = null;
    for (const part of chunks(targets, MAX_ACTION_MESSAGES)) {
      try {
        const refs = part.map((item) => ({ mailboxId: item.mailboxId, messageId: item.id }));
        const result = await ipc.applyAction(action, refs, crypto.randomUUID());
        if (result.batch !== null) batches.push(result.batch);
        if (result.queued > 0 || result.cancelled > 0) recorded = true;
        done += part.length;
      } catch (e) {
        failure = toCoreError(e);
        break;
      }
    }
    remember(batches);
    if (recorded && !options.silent) {
      const undoable = action.type !== "deletePermanently" && batches.length > 0;
      push({
        kind: "done",
        action: actionKind(action),
        count: done,
        ...(options.detail !== undefined && { detail: options.detail }),
        undo: undoable ? { accountId: account.id, batches } : null,
      });
    }
    if (failure && !options.ownErrors?.includes(failure.code)) {
      handleError(failure, action, items, options.errorContext ?? "action", options.exact === true);
    }
    if (leaves) {
      // The core changed its local state before it answered: the reloaded list no longer has
      // the rows (or has them back after a failure), so hiding them is not needed any more.
      // Not exact: the folder's other lists (stage 9: sorted inbox, Later, Pinned, Tasks) too.
      await queryClient.invalidateQueries({ queryKey: messagesQueryKey(account.id, mailbox.id) });
      hide(ids, false);
    }
    return failure ? { ok: false, error: failure } : { ok: true };
  };

  /** The actions for list rows (`exact` false) or for the one message of the reading pane. */
  const makeHost = (exact: boolean): ActionHost => ({
    apply: (action, items) => {
      void apply(action, items, exact ? { exact } : {});
    },
    confirmDeletePermanently: (items) => {
      if (!account.writesEnabled) {
        push({ kind: "readOnly", accountId: account.id });
        return;
      }
      setOverlay({ kind: "confirmDelete", items, exact });
    },
    openMove: (items) => {
      setOverlay({ kind: "move", ids: items.map((item) => item.id), exact });
    },
    openTags: (items) => {
      setOverlay({ kind: "tags", ids: items.map((item) => item.id), exact });
    },
    undo: () => {
      if (lastUndo) void undoAction(queryClient, lastUndo);
    },
    toggleSelect: () => {
      list.current?.toggleCursor();
    },
    selectAll: () => {
      list.current?.selectAll();
    },
    ...(exact ? comfort.exactHost : comfort.host),
  });
  const host = makeHost(false);
  // Without conversations both act on what they are given.
  const messageHost = conversations ? makeHost(true) : host;

  /** The answer to `missing_mailbox`: this folder has the role from now on; then the action runs. */
  const chooseFolderForRole = async (target: Mailbox): Promise<CoreError | null> => {
    if (!missing) return null;
    try {
      const mailboxes = await ipc.setMailboxRole(target.id, missing.role);
      queryClient.setQueryData(mailboxesQueryKey(account.id), mailboxes);
    } catch (e) {
      return toCoreError(e);
    }
    const { action, items, exact } = missing;
    setMissing(null);
    await apply(action, items, { exact: exact === true });
    return null;
  };

  /** Or a new folder: queued like any change, the action runs once the server created it. */
  const createFolderForRole = async (name: string): Promise<CoreError | null> => {
    if (!missing) return null;
    const pending = missing;
    try {
      await ipc.createMailbox(account.id, name, pending.role);
    } catch (e) {
      return toCoreError(e);
    }
    setMissing({ ...pending, creating: name });
    waiting.current?.abort();
    const controller = new AbortController();
    waiting.current = controller;
    try {
      await waitForRole(queryClient, account, pending.role, controller.signal);
    } catch {
      return null; // cancelled by the user or the view closed
    }
    setMissing(null);
    await apply(pending.action, pending.items, { exact: pending.exact === true });
    return null;
  };

  const cancelMissing = () => {
    waiting.current?.abort();
    setMissing(null);
  };

  return {
    apply,
    host,
    /** Decision 0039: the toolbar of the reading pane acts on its one message. */
    messageHost,
    /** Whether the rows are conversations (actions on them act on the conversation). */
    conversations,
    hiddenIds,
    overlay,
    closeOverlay: () => {
      setOverlay(null);
    },
    missing,
    chooseFolderForRole,
    createFolderForRole,
    cancelMissing,
    canUndo: lastUndo !== null,
    comfort,
  };
}

export type ActionController = ReturnType<typeof useActionController>;
