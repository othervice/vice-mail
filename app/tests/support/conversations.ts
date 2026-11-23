// Decision 0039 part of the fake core: the sorted inbox, its filtered lists and "unread first" as
// conversations, and the messages an action on conversation rows touches. The fake answers the
// message lists of the earlier fakes and groups them by thread, one row per thread at its newest
// message, the way the core does.
import type {
  Address,
  ConversationIntent,
  ConversationSummary,
  InboxPage,
  InboxRow,
  MessageListItem,
  MessagePage,
  MessageRef,
  ThreadListItem,
  ThreadPage,
  UnreadFirstPage,
  UnreadFirstThreadPage,
} from "../../src/services/ipc";

export interface ConversationFakeOptions {
  /** The thread of each message id; a message without an entry is a conversation of its own. */
  threadOf?: Record<number, number>;
}

interface Deps {
  /** Every message the fake core holds, in every mailbox. */
  messages: () => MessageListItem[];
  /** The answer of an earlier fake to a message list command, without recording a call. */
  answer: (cmd: string, args: Record<string, unknown>) => unknown;
}

const MAX_PARTICIPANTS = 4;

type Reach = "newest" | "listed" | "everywhere";

/** The core's `ConversationIntent::reach` (decision 0039). */
export function reachOf(intent: ConversationIntent): Reach {
  switch (intent.kind) {
    case "action":
      switch (intent.action.type) {
        case "markRead":
        case "unflag":
          return "everywhere";
        case "markUnread":
        case "flag":
          return "newest";
        default:
          return "listed";
      }
    case "pin":
      return intent.pinned ? "newest" : "everywhere";
    case "unsnooze":
      return "everywhere";
    case "task":
      return intent.clear ? "everywhere" : "newest";
    case "readingList":
      return intent.on ? "newest" : "everywhere";
    case "block":
      return "newest";
    case "snooze":
      return "listed";
  }
}

export function createConversationFake(options: ConversationFakeOptions, deps: Deps) {
  const threadOf = (id: number): number | null => options.threadOf?.[id] ?? null;
  const key = (item: MessageListItem) => threadOf(item.id) ?? -item.id;
  const members = (item: MessageListItem): MessageListItem[] => {
    const thread = threadOf(item.id);
    if (thread === null) return [item];
    const seen = new Set<number>();
    return deps
      .messages()
      .filter((m) => threadOf(m.id) === thread && !seen.has(m.id) && (seen.add(m.id), true))
      .sort((a, b) => a.id - b.id);
  };

  const summarize = (item: MessageListItem): ThreadListItem => {
    const all = members(item);
    const unread = new Set(
      deps
        .messages()
        .filter((m) => m.unread && all.some((a) => a.id === m.id))
        .map((m) => m.id),
    );
    const participants: Address[] = [];
    for (const m of all) {
      if (m.from && participants.length < MAX_PARTICIPANTS && !participants.some((p) => p.email.toLowerCase() === m.from?.email.toLowerCase())) {
        participants.push(m.from);
      }
    }
    return {
      message: item,
      threadId: threadOf(item.id),
      messageCount: Math.max(1, all.length),
      unreadCount: Math.max(unread.size, item.unread ? 1 : 0),
      lastActivity: Math.max(item.date, ...all.map((m) => m.date)),
      participants,
      hasAttachments: all.some((m) => m.hasAttachments) || item.hasAttachments,
      flagged: all.some((m) => m.flagged) || item.flagged,
    };
  };

  const summary = (item: ThreadListItem): ConversationSummary => ({
    messageId: item.message.id,
    threadId: item.threadId,
    messageCount: item.messageCount,
    unreadCount: item.unreadCount,
    lastActivity: item.lastActivity,
    participants: item.participants,
    hasAttachments: item.hasAttachments,
    flagged: item.flagged,
  });

  /** Keeps the newest message of each thread (lists come newest first). */
  const grouped = (items: readonly MessageListItem[], taken = new Set<number>()): MessageListItem[] =>
    items.filter((item) => {
      const k = key(item);
      if (taken.has(k)) return false;
      taken.add(k);
      return true;
    });

  function inbox(args: Record<string, unknown>): unknown {
    const answer = deps.answer("list_inbox", args);
    if (answer === undefined || answer instanceof Promise) return answer;
    const page = answer as InboxPage;
    const taken = new Set<number>();
    const summaries = new Map<number, ConversationSummary>();
    const keep = (items: readonly MessageListItem[]) => {
      const kept = grouped(items, taken);
      for (const item of kept) summaries.set(item.id, summary(summarize(item)));
      return kept;
    };
    const top = page.top && {
      ...page.top,
      pinned: keep(page.top.pinned),
      returned: keep(page.top.returned),
      // Priority conversations stand in the rows as well (the UI shows them once).
      priority: grouped(page.top.priority),
    };
    for (const item of top?.priority ?? []) summaries.set(item.id, summary(summarize(item)));
    const rows: InboxRow[] = page.rows.flatMap((row): InboxRow[] => {
      if (row.kind === "message") return keep([row.item]).map((item) => ({ ...row, item }));
      const kept = keep(row.members);
      return kept.length > 0 ? [{ ...row, members: kept }] : [];
    });
    return { ...page, rows, top, conversations: [...summaries.values()] } satisfies InboxPage;
  }

  function threadPage(items: readonly MessageListItem[], nextCursor: ThreadPage["nextCursor"]): ThreadPage {
    return { items: grouped(items).map(summarize), nextCursor };
  }

  function targets(args: Record<string, unknown>): MessageListItem[] {
    const rows = args["rows"] as MessageRef[];
    const reach = reachOf(args["intent"] as ConversationIntent);
    const all = deps.messages();
    const out: MessageListItem[] = [];
    const seen = new Set<string>();
    for (const row of rows) {
      const item = all.find((m) => m.id === row.messageId && m.mailboxId === row.mailboxId) ?? all.find((m) => m.id === row.messageId);
      if (!item) continue;
      const touched =
        reach === "newest" ? [item] : members(item).filter((m) => reach === "everywhere" || m.mailboxId === row.mailboxId);
      for (const m of touched) {
        const id = `${String(m.id)}/${String(m.mailboxId)}`;
        if (seen.has(id)) continue;
        seen.add(id);
        out.push(m);
      }
    }
    return out.sort((a, b) => b.id - a.id);
  }

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "list_inbox_conversations":
        return inbox(args);
      case "list_inbox_conversations_filtered": {
        const answer = deps.answer("list_inbox_filtered", args);
        if (answer === undefined || answer instanceof Promise) return answer;
        const page = answer as MessagePage;
        return threadPage(page.items, page.nextCursor);
      }
      case "list_unread_first_conversations": {
        const answer = deps.answer("list_unread_first", args);
        if (answer === undefined || answer instanceof Promise) return answer;
        const page = answer as UnreadFirstPage;
        return { items: grouped(page.items).map(summarize), nextCursor: page.nextCursor } satisfies UnreadFirstThreadPage;
      }
      case "conversation_targets":
        return targets(args);
      default:
        return undefined;
    }
  }

  return { handle, summarize };
}
