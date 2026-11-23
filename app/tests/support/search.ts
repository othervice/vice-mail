// Stage 7 part of the fake core: search, conversations, unified inbox, account groups, hidden
// accounts and identities, with the rules of the stage 7 IPC contract.
import { messageRow } from "./comfort";
import type {
  Account,
  AccountGroup,
  IdentityInfo,
  IdentityInput,
  InboxCursor,
  InboxPage,
  InboxScope,
  IpcError,
  Mailbox,
  MessageListItem,
  SearchHit,
  SearchIndexStatus,
  SearchPage,
  SearchRequest,
  ServerSearchResult,
  TextRange,
  ThreadListItem,
  ThreadView,
  UnifiedView,
} from "../../src/services/ipc";

export interface SearchFakeOptions {
  /** Answers `search`; default: every word of the query in subject, sender or preview. */
  search?: (request: SearchRequest) => SearchPage;
  /** Answers `searchOnServer`; default: nothing found. */
  serverSearch?: (request: SearchRequest) => ServerSearchResult;
  /** Conversations by mailbox id; default: every message of the folder is its own thread. */
  threads?: Record<number, ThreadListItem[]>;
  /** `getThread` answers by message id; default: the message alone. */
  threadViews?: Record<number, ThreadView>;
  groups?: AccountGroup[];
  hidden?: string[];
  /** Identities of every account; the default of each account first. */
  identities?: IdentityInfo[];
  index?: SearchIndexStatus;
}

export interface SearchFakeState {
  readonly groups: AccountGroup[];
  readonly hidden: string[];
  readonly identities: IdentityInfo[];
}

interface CoreData {
  accounts: () => Account[];
  mailboxes: () => Mailbox[];
  messages: (mailboxId: number) => MessageListItem[];
}

const READY: SearchIndexStatus = { state: "ready", percent: 100 };

function error(code: IpcError["code"]): Promise<never> {
  // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
  return Promise.reject({ code, message: `English fallback for ${code}`, detail: null } satisfies IpcError);
}

/** Every place of `word` in `text`, case-insensitive (UTF-16 indices). */
function find(text: string, word: string): TextRange[] {
  const ranges: TextRange[] = [];
  const haystack = text.toLowerCase();
  const needle = word.toLowerCase();
  if (!needle) return ranges;
  for (let at = haystack.indexOf(needle); at >= 0; at = haystack.indexOf(needle, at + needle.length)) {
    ranges.push({ start: at, end: at + needle.length });
  }
  return ranges;
}

/** A hit the way the core builds one, with highlights for `words`. */
export function makeHit(message: MessageListItem, words: readonly string[] = [], overrides: Partial<SearchHit> = {}): SearchHit {
  const sender = message.from ? message.from.name?.trim() || message.from.email : "";
  return {
    message,
    mailboxIds: [message.mailboxId],
    highlights: {
      subject: words.flatMap((w) => find(message.subject, w)),
      from: words.flatMap((w) => find(sender, w)),
      preview: words.flatMap((w) => find(message.preview ?? "", w)),
    },
    bodyLocal: true,
    ...overrides,
  };
}

export function searchPage(overrides: Partial<SearchPage> = {}): SearchPage {
  return { hits: [], nextCursor: null, problems: [], terms: [], matchesNothing: false, index: READY, ...overrides };
}

function page<T extends { id: number }>(all: readonly T[], cursor: number | null, limit: number) {
  const start = cursor === null ? 0 : all.findIndex((m) => m.id < cursor);
  const items = start < 0 ? [] : all.slice(start, start + limit);
  const last = items[items.length - 1];
  const more = start >= 0 && start + limit < all.length;
  return { items, nextCursor: more && last ? last.id : null };
}

export function createSearchFake(options: SearchFakeOptions, core: CoreData) {
  const groups: AccountGroup[] = [...(options.groups ?? [])];
  const hidden: string[] = [...(options.hidden ?? [])];
  const identities: IdentityInfo[] = [...(options.identities ?? [])];
  let nextGroup = 1;
  let nextIdentity = Math.max(0, ...identities.map((i) => i.id)) + 1;

  const visibleAccounts = (view: UnifiedView): Account[] => {
    const members = view.kind === "group" ? groups.find((g) => g.id === view.groupId)?.accountIds : undefined;
    return core
      .accounts()
      .filter((a) => !hidden.includes(a.id) && (view.kind === "all" || (members ?? []).includes(a.id)));
  };
  const inboxMessages = (view: UnifiedView): MessageListItem[] =>
    visibleAccounts(view)
      .flatMap((account) => {
        const inbox = core.mailboxes().find((m) => m.accountId === account.id && m.role === "inbox");
        return inbox ? core.messages(inbox.id) : [];
      })
      .sort((a, b) => b.id - a.id);

  const scopeMessages = (request: SearchRequest): MessageListItem[] => {
    const { scope } = request;
    const mailboxes = core.mailboxes().filter((m) => {
      if (scope.kind === "mailbox") return m.id === scope.mailboxId;
      if (scope.kind === "account") return m.accountId === scope.accountId;
      return visibleAccounts(scope.kind === "group" ? { kind: "group", groupId: scope.groupId } : { kind: "all" }).some(
        (a) => a.id === m.accountId,
      );
    });
    return mailboxes.flatMap((m) => core.messages(m.id)).sort((a, b) => b.id - a.id);
  };

  const defaultSearch = (request: SearchRequest): SearchPage => {
    const words = request.query.split(/\s+/).filter((w) => w && !w.includes(":"));
    if (words.length === 0) return searchPage({ index: options.index ?? READY });
    const matches = scopeMessages(request).filter((m) =>
      words.every((w) =>
        [m.subject, m.from?.name ?? "", m.from?.email ?? "", m.preview ?? ""].some((text) => find(text, w).length > 0),
      ),
    );
    const { items, nextCursor } = page(matches, request.cursor, request.limit);
    return searchPage({ hits: items.map((m) => makeHit(m, words)), nextCursor, index: options.index ?? READY });
  };

  const identitiesOf = (accountId: string) =>
    identities.filter((i) => i.accountId === accountId).sort((a, b) => Number(b.isDefault) - Number(a.isDefault));
  const validIdentity = (input: IdentityInput, accountId: string, except: number | null) =>
    /^[^@\s]+@[^@\s]+$/.test(input.email) &&
    !identities.some((i) => i.accountId === accountId && i.id !== except && i.email.toLowerCase() === input.email.toLowerCase());

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "search":
        return (options.search ?? defaultSearch)(args["request"] as SearchRequest);
      case "search_on_server":
        return (
          options.serverSearch?.(args["request"] as SearchRequest) ?? {
            hits: [],
            truncated: false,
            searched: [],
            failed: [],
            notSynced: 0,
            problems: [],
          }
        );
      case "get_search_index_status":
      case "rebuild_search_index":
        return options.index ?? READY;
      case "list_threads": {
        const mailboxId = args["mailboxId"] as number;
        const threads =
          options.threads?.[mailboxId] ??
          core.messages(mailboxId).map((message) => ({
            message,
            threadId: null,
            messageCount: 1,
            unreadCount: message.unread ? 1 : 0,
            lastActivity: message.date,
          }));
        const paged = page(
          threads.map((thread) => ({ id: thread.message.id, thread })),
          args["cursor"] as number | null,
          args["limit"] as number,
        );
        return { items: paged.items.map((entry) => entry.thread), nextCursor: paged.nextCursor };
      }
      case "list_inbox": {
        // Stage 9's sorted inbox with the unified scope: the inboxes of the visible accounts
        // (or a group's), chronological; one account's inbox stays with the stage 9 fake.
        const scope = args["scope"] as InboxScope;
        if (scope.kind !== "unified") return undefined;
        if (scope.view.kind === "group" && !groups.some((g) => scope.view.kind === "group" && g.id === scope.view.groupId)) {
          return error("not_found");
        }
        const cursor = args["cursor"] as InboxCursor | null;
        const paged = page(inboxMessages(scope.view), cursor?.before ?? null, args["limit"] as number);
        return {
          rows: paged.items.map((item) => messageRow(item)),
          nextCursor: paged.nextCursor === null ? null : { before: paged.nextCursor, bundles: null },
          top: null,
          categories: false,
        } satisfies InboxPage;
      }
      case "list_inbox_filtered": {
        const scope = args["scope"] as InboxScope;
        return scope.kind === "unified" ? { items: [], nextCursor: null } : undefined;
      }
      case "list_unified_threads": {
        const view = args["view"] as UnifiedView;
        const paged = page(inboxMessages(view), args["cursor"] as number | null, args["limit"] as number);
        return {
          items: paged.items.map((message) => ({
            message,
            threadId: null,
            messageCount: 1,
            unreadCount: message.unread ? 1 : 0,
            lastActivity: message.date,
          })),
          nextCursor: paged.nextCursor,
        };
      }
      case "get_thread": {
        const messageId = args["messageId"] as number;
        const fixed = options.threadViews?.[messageId];
        if (fixed) return fixed;
        const message = core
          .mailboxes()
          .flatMap((m) => core.messages(m.id))
          .find((m) => m.id === messageId);
        if (!message) return error("not_found");
        return { threadId: null, messages: [{ message, mailboxIds: [message.mailboxId] }], total: 1 };
      }
      case "list_unified_inbox": {
        const view = args["view"] as UnifiedView;
        if (view.kind === "group" && !groups.some((g) => g.id === view.groupId)) return error("not_found");
        return page(inboxMessages(view), args["cursor"] as number | null, args["limit"] as number);
      }
      case "list_account_groups":
        return groups.map((g) => ({ ...g, accountIds: [...g.accountIds] }));
      case "create_account_group":
      case "update_account_group": {
        const name = (args["name"] as string).trim();
        const accountIds = args["accountIds"] as string[];
        if (name.length < 1 || name.length > 100) return error("invalid_argument");
        if (accountIds.some((id) => !core.accounts().some((a) => a.id === id))) return error("not_found");
        if (cmd === "create_account_group") {
          const group: AccountGroup = {
            id: `00000000-0000-4000-8000-${String(nextGroup++).padStart(12, "0")}`,
            name,
            accountIds,
            position: groups.length,
          };
          groups.push(group);
          return group;
        }
        const index = groups.findIndex((g) => g.id === args["groupId"]);
        const current = groups[index];
        if (!current) return error("not_found");
        const next = { ...current, name, accountIds };
        groups[index] = next;
        return next;
      }
      case "delete_account_group": {
        const index = groups.findIndex((g) => g.id === args["groupId"]);
        if (index < 0) return error("not_found");
        groups.splice(index, 1);
        return null;
      }
      case "list_hidden_accounts":
        return [...hidden];
      case "set_account_hidden": {
        const accountId = args["accountId"] as string;
        if (!core.accounts().some((a) => a.id === accountId)) return error("not_found");
        const at = hidden.indexOf(accountId);
        if (args["hidden"] === true && at < 0) hidden.push(accountId);
        if (args["hidden"] === false && at >= 0) hidden.splice(at, 1);
        return null;
      }
      case "list_identities":
        return identitiesOf(args["accountId"] as string).map((i) => ({ ...i }));
      case "add_identity": {
        const accountId = args["accountId"] as string;
        const input = args["identity"] as IdentityInput;
        if (!core.accounts().some((a) => a.id === accountId)) return error("not_found");
        if (!validIdentity(input, accountId, null)) return error("invalid_argument");
        const identity: IdentityInfo = {
          id: nextIdentity++,
          accountId,
          ...input,
          isDefault: identitiesOf(accountId).length === 0,
        };
        identities.push(identity);
        return identity;
      }
      case "update_identity": {
        const index = identities.findIndex((i) => i.id === args["identityId"]);
        const current = identities[index];
        if (!current) return error("not_found");
        const input = args["identity"] as IdentityInput;
        if (!validIdentity(input, current.accountId, current.id)) return error("invalid_argument");
        const next = { ...current, ...input };
        identities[index] = next;
        return next;
      }
      case "set_default_identity": {
        const target = identities.find((i) => i.id === args["identityId"]);
        if (!target) return error("not_found");
        for (const [index, identity] of identities.entries()) {
          if (identity.accountId === target.accountId) identities[index] = { ...identity, isDefault: identity.id === target.id };
        }
        return identitiesOf(target.accountId).map((i) => ({ ...i }));
      }
      case "delete_identity": {
        const index = identities.findIndex((i) => i.id === args["identityId"]);
        const current = identities[index];
        if (!current) return error("not_found");
        if (current.isDefault) return error("invalid_argument");
        identities.splice(index, 1);
        return null;
      }
      case "get_reply_identity": {
        const first = identities.find((i) => i.isDefault);
        return first ?? error("not_found");
      }
      default:
        return undefined;
    }
  }

  const state: SearchFakeState = { groups, hidden, identities };
  return { handle, state };
}
