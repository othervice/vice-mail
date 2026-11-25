// Stage 16 part of the fake core (decision 0033): the reading list, unread views, favourite
// folders and folder colours, moving and copying between accounts and blocked senders, with the
// command names and argument shapes of the IPC contract.
import type {
  AccountColor,
  ActionResult,
  BlockedSender,
  InboxFilter,
  InboxScope,
  IpcError,
  Mailbox,
  MarkScope,
  MessageListItem,
  MessagePage,
  MessageRef,
  TransferMode,
  UnreadFirstCursor,
  UnreadFirstPage,
} from "../../src/services/ipc";

export interface ParityFakeOptions {
  /** Message ids on the reading list, most recently added first. */
  readingList?: number[];
  /** View keys with "Unread first" on. */
  unreadFirstViews?: string[];
  blockedSenders?: BlockedSender[];
}

export interface ParityFakeState {
  readonly readingList: readonly number[];
  readonly unreadFirstViews: readonly string[];
  readonly blockedSenders: readonly BlockedSender[];
  /** Every `transferMessages` call that was accepted. */
  readonly transfers: readonly { messages: MessageRef[]; targetMailboxId: number; mode: TransferMode; requestId: string }[];
}

interface ParityDeps {
  messages: () => MessageListItem[];
  mailboxes: () => Mailbox[];
  setMailboxes: (mailboxes: Mailbox[]) => void;
  writesEnabled: (accountId: string) => boolean;
  reject: (code: IpcError["code"], detail?: string | null) => Promise<never>;
}

function scopeMatches(item: MessageListItem, scope: MarkScope): boolean {
  return scope.kind !== "account" || item.accountId === scope.accountId;
}

export function createParityFake(options: ParityFakeOptions, deps: ParityDeps) {
  let readingList = [...(options.readingList ?? [])];
  let unreadFirstViews = [...(options.unreadFirstViews ?? [])];
  let blockedSenders = [...(options.blockedSenders ?? [])];
  const transfers: ParityFakeState["transfers"][number][] = [];
  let nextBatch = 9000;

  const withMarks = (item: MessageListItem): MessageListItem =>
    readingList.includes(item.id) ? { ...item, readingList: true } : item;

  /** The messages of an inbox scope, newest first, without hidden mail of blocked senders. */
  function scopeItems(scope: InboxScope): MessageListItem[] {
    const mailboxes = deps.mailboxes();
    const inScope = (item: MessageListItem) => {
      if (scope.kind === "mailbox") return item.mailboxId === scope.mailboxId;
      const inbox = mailboxes.find((m) => m.id === item.mailboxId);
      return inbox?.role === "inbox";
    };
    const blocked = new Set(blockedSenders.map((b) => b.address));
    return deps
      .messages()
      .filter(inScope)
      .filter((item) => !blocked.has(item.from?.email.toLowerCase() ?? ""))
      .sort((a, b) => b.date - a.date)
      .map(withMarks);
  }

  function favorites(): Mailbox[] {
    return deps
      .mailboxes()
      .filter((m) => m.favoritePosition !== undefined && m.favoritePosition !== null)
      .sort((a, b) => (a.favoritePosition ?? 0) - (b.favoritePosition ?? 0));
  }

  function updateMailbox(mailboxId: number, change: (mailbox: Mailbox) => Mailbox): boolean {
    const all = deps.mailboxes();
    if (!all.some((m) => m.id === mailboxId)) return false;
    deps.setMailboxes(all.map((m) => (m.id === mailboxId ? change(m) : m)));
    return true;
  }

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "set_reading_list": {
        const ids = args["messageIds"] as number[];
        if (ids.length === 0 || ids.length > 10_000) return deps.reject("invalid_argument", "messages");
        if (args["on"] as boolean) {
          readingList = [...ids.filter((id) => !readingList.includes(id)), ...readingList];
        } else {
          readingList = readingList.filter((id) => !ids.includes(id));
        }
        return null;
      }
      case "list_reading_list": {
        const scope = args["scope"] as MarkScope;
        const all = deps.messages();
        return readingList.flatMap((id) => {
          const item = all.find((m) => m.id === id);
          return item && scopeMatches(item, scope) ? [withMarks(item)] : [];
        });
      }
      case "list_inbox_filtered": {
        // Only the stage 16 filter; the others are the stage 9 fake's.
        if ((args["filter"] as InboxFilter).kind !== "unread") return undefined;
        const items = scopeItems(args["scope"] as InboxScope).filter((item) => item.unread);
        return { items, nextCursor: null } satisfies MessagePage;
      }
      case "list_unread_first": {
        const cursor = args["cursor"] as UnreadFirstCursor | null;
        if (cursor) return { items: [], nextCursor: null } satisfies UnreadFirstPage;
        const items = scopeItems(args["scope"] as InboxScope);
        return {
          items: [...items.filter((item) => item.unread), ...items.filter((item) => !item.unread)],
          nextCursor: null,
        } satisfies UnreadFirstPage;
      }
      case "get_unread_first_views":
        return [...unreadFirstViews];
      case "set_unread_first_view": {
        const view = args["view"] as string;
        if (view.length === 0 || view.length > 200) return deps.reject("invalid_argument", "view");
        unreadFirstViews = unreadFirstViews.filter((v) => v !== view);
        if (args["enabled"] as boolean) unreadFirstViews.push(view);
        return [...unreadFirstViews];
      }
      case "set_mailbox_favorite": {
        const mailboxId = args["mailboxId"] as number;
        const favorite = args["favorite"] as boolean;
        const end = Math.max(-1, ...favorites().map((m) => m.favoritePosition ?? 0)) + 1;
        const ok = updateMailbox(mailboxId, (m) => {
          if (!favorite) {
            const rest = { ...m };
            delete rest.favoritePosition;
            return rest;
          }
          return m.favoritePosition === undefined || m.favoritePosition === null ? { ...m, favoritePosition: end } : m;
        });
        return ok ? null : deps.reject("not_found");
      }
      case "set_mailbox_color": {
        const color = args["color"] as AccountColor | null;
        const ok = updateMailbox(args["mailboxId"] as number, (m) => {
          if (color === null) {
            const rest = { ...m };
            delete rest.color;
            return rest;
          }
          return { ...m, color };
        });
        return ok ? null : deps.reject("not_found");
      }
      case "reorder_favorite_mailboxes": {
        const ids = args["mailboxIds"] as number[];
        const rest = favorites()
          .map((m) => m.id)
          .filter((id) => !ids.includes(id));
        const order = [...ids, ...rest];
        deps.setMailboxes(
          deps.mailboxes().map((m) => {
            const position = order.indexOf(m.id);
            return position >= 0 && m.favoritePosition !== undefined && m.favoritePosition !== null
              ? { ...m, favoritePosition: position }
              : m;
          }),
        );
        return favorites();
      }
      case "list_favorite_mailboxes":
        return favorites();
      case "transfer_messages": {
        const messages = args["messages"] as MessageRef[];
        const targetMailboxId = args["targetMailboxId"] as number;
        const mode = args["mode"] as TransferMode;
        const requestId = args["requestId"] as string;
        const target = deps.mailboxes().find((m) => m.id === targetMailboxId);
        if (!target?.selectable) return deps.reject("invalid_argument", "target");
        if (messages.length === 0 || messages.length > 500) return deps.reject("invalid_argument", "messages");
        const sources = messages.map((ref) => deps.mailboxes().find((m) => m.id === ref.mailboxId));
        // Stage 21 (decision 0064): a copy may go to another folder of the same account.
        if (sources.some((m) => !m || (m.accountId === target.accountId && (mode === "move" || m.id === target.id)))) {
          return deps.reject("invalid_argument", "same account");
        }
        if (!deps.writesEnabled(target.accountId)) return deps.reject("read_only");
        if (mode === "move" && sources.some((m) => m && !deps.writesEnabled(m.accountId))) {
          return deps.reject("read_only");
        }
        transfers.push({ messages, targetMailboxId, mode, requestId });
        return { batch: nextBatch++, queued: messages.length, cancelled: 0 } satisfies ActionResult;
      }
      case "block_sender": {
        const address = (args["address"] as string).trim().toLowerCase();
        if (!/^[^\s@<>]+@[^\s@<>]+$/.test(address)) return deps.reject("invalid_argument", "address");
        if (!blockedSenders.some((b) => b.address === address)) {
          blockedSenders = [...blockedSenders, { address, createdAt: Date.now() }].sort((a, b) =>
            a.address.localeCompare(b.address),
          );
        }
        return [...blockedSenders];
      }
      case "unblock_sender": {
        const address = (args["address"] as string).trim().toLowerCase();
        blockedSenders = blockedSenders.filter((b) => b.address !== address);
        return [...blockedSenders];
      }
      case "list_blocked_senders":
        return [...blockedSenders];
      default:
        return undefined;
    }
  }

  const state: ParityFakeState = {
    get readingList() {
      return readingList;
    },
    get unreadFirstViews() {
      return unreadFirstViews;
    },
    get blockedSenders() {
      return blockedSenders;
    },
    transfers,
  };

  return { handle, state };
}
