// Stage 19, task A part of the fake core (decisions 0061, 0062): sorted and filtered list views,
// counts, tags, the order per view and the table settings, with the command names and argument
// shapes of the IPC contract. Sorting folds like the core (lower case, accents removed, leading
// punctuation dropped; ties by date in the same direction); the text filter matches sender,
// subject and preview as words.
import type {
  InboxScope,
  ListOrder,
  ListTableSettings,
  ListViewRequest,
  Mailbox,
  MessageListItem,
  ViewCursor,
  ViewMessagePage,
  ViewOrder,
  ViewThreadPage,
} from "../../src/services/ipc";
import { DEFAULT_LIST_TABLE } from "../../src/features/views/table";

export interface ViewsFakeOptions {
  orders?: ViewOrder[];
  table?: ListTableSettings;
  /** Senders (lower-case addresses) that are contacts. */
  contacts?: string[];
}

export interface ViewsFakeState {
  readonly orders: readonly ViewOrder[];
  readonly table: ListTableSettings;
}

interface ViewsDeps {
  messages: () => MessageListItem[];
  mailboxes: () => Mailbox[];
}

export function fold(text: string): string {
  return text
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/ß/g, "ss")
    .replace(/^[^\p{L}\p{N}]+/u, "")
    .trim();
}

function keyOf(item: MessageListItem, order: ListOrder): string | number {
  const person = (name: string | null | undefined, email: string | undefined) => fold(name?.trim() || email || "");
  switch (order.key) {
    case "date":
      return item.id;
    case "sender":
      return person(item.from?.name, item.from?.email);
    case "recipient":
      return person(item.to[0]?.name, item.to[0]?.email);
    case "subject":
      return fold(item.subject.replace(/^((re|aw|fwd|wg)\s*:\s*)+/i, ""));
    case "size":
      return item.size ?? 0;
  }
}

function compare(a: MessageListItem, b: MessageListItem, order: ListOrder): number {
  const ka = keyOf(a, order);
  const kb = keyOf(b, order);
  const byKey = typeof ka === "number" && typeof kb === "number" ? ka - kb : String(ka) < String(kb) ? -1 : String(ka) > String(kb) ? 1 : 0;
  const result = byKey !== 0 ? byKey : a.id - b.id;
  return order.ascending ? result : -result;
}

export function createViewsFake(options: ViewsFakeOptions, deps: ViewsDeps) {
  let orders = [...(options.orders ?? [])];
  let table = structuredClone(options.table ?? DEFAULT_LIST_TABLE);
  const contacts = new Set(options.contacts ?? []);

  function inScope(item: MessageListItem, scope: InboxScope): boolean {
    if (scope.kind === "mailbox") return item.mailboxId === scope.mailboxId;
    return deps.mailboxes().find((m) => m.id === item.mailboxId)?.role === "inbox";
  }

  function matching(request: ListViewRequest): MessageListItem[] {
    const filter = request.filter ?? { unread: false, flagged: false, attachment: false, contacts: false, tag: null, text: "", monthFirst: false };
    const words = fold(filter.text ?? "")
      .split(/\s+/)
      .filter((word) => word.length > 0);
    return deps
      .messages()
      .filter((item) => inScope(item, request.scope))
      .filter((item) => !filter.unread || item.unread)
      .filter((item) => !filter.flagged || item.flagged)
      .filter((item) => !filter.attachment || item.hasAttachments)
      .filter((item) => !filter.contacts || contacts.has(item.from?.email.toLowerCase() ?? ""))
      .filter((item) => filter.tag === null || filter.tag === undefined || item.keywords.some((k) => k.toLowerCase() === filter.tag?.toLowerCase()))
      .filter((item) => {
        const text = fold([item.from?.name, item.from?.email, item.subject, item.preview].filter(Boolean).join(" "));
        return words.every((word) => text.split(/[^\p{L}\p{N}@.]+/u).some((part) => part.startsWith(word)));
      });
  }

  function page(request: ListViewRequest, cursor: ViewCursor | null, limit: number) {
    const sorted = matching(request).sort((a, b) => compare(a, b, request.order));
    const start = cursor ? sorted.findIndex((item) => item.id === cursor.id) + 1 : 0;
    const items = sorted.slice(start, start + limit);
    const last = items.at(-1);
    const more = start + limit < sorted.length;
    const key = last && keyOf(last, request.order);
    const nextCursor: ViewCursor | null =
      more && last
        ? { text: typeof key === "string" ? key : null, number: request.order.key === "size" ? (last.size ?? 0) : null, id: last.id }
        : null;
    return { items, nextCursor };
  }

  const state: ViewsFakeState = {
    get orders() {
      return orders;
    },
    get table() {
      return table;
    },
  };

  return {
    state,
    handle(cmd: string, args: Record<string, unknown>): unknown {
      switch (cmd) {
        case "list_view_messages": {
          const request = args["request"] as ListViewRequest;
          return page(request, args["cursor"] as ViewCursor | null, args["limit"] as number) satisfies ViewMessagePage;
        }
        case "list_view_conversations": {
          const request = args["request"] as ListViewRequest;
          const { items, nextCursor } = page(request, args["cursor"] as ViewCursor | null, args["limit"] as number);
          return {
            items: items.map((message) => ({
              message,
              threadId: null,
              messageCount: 1,
              unreadCount: message.unread ? 1 : 0,
              lastActivity: message.date,
              participants: message.from ? [message.from] : [],
              hasAttachments: message.hasAttachments,
              flagged: message.flagged,
              subject: message.subject,
            })),
            nextCursor,
          } satisfies ViewThreadPage;
        }
        case "count_list_view":
          return matching(args["request"] as ListViewRequest).length;
        case "list_view_tags": {
          const scope = args["scope"] as InboxScope;
          const tags = new Map<string, string>();
          for (const item of deps.messages().filter((m) => inScope(m, scope))) {
            for (const keyword of item.keywords) if (!keyword.startsWith("$") && !tags.has(keyword.toLowerCase())) tags.set(keyword.toLowerCase(), keyword);
          }
          return [...tags.values()].sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()));
        }
        case "get_list_orders":
          return [...orders];
        case "set_list_order": {
          const view = args["view"] as string;
          const order = args["order"] as ListOrder | null;
          orders = orders.filter((entry) => entry.view !== view);
          if (order && !(order.key === "date" && !order.ascending)) orders.push({ view, order });
          return [...orders];
        }
        case "get_list_table":
          return structuredClone(table);
        case "set_list_table":
          table = structuredClone(args["settings"] as ListTableSettings);
          return structuredClone(table);
        default:
          return undefined;
      }
    },
  };
}
