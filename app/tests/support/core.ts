import { mockIPC } from "@tauri-apps/api/mocks";
import type {
  Account,
  AccountSettings,
  AccountStatus,
  AppInfo,
  Capabilities,
  DiscoveryResult,
  IpcError,
  Mailbox,
  MailboxRole,
  MessageListItem,
  MessagePage,
  Preferences,
} from "../../src/services/ipc";

export const APP_INFO: AppInfo = {
  version: "0.1.0",
  commit: "1e235a5",
  buildDate: "2026-11-22T18:00:00Z",
  platform: "macos",
};

/** Rejection values a command can fail with: a typed IpcError or a framework string. */
export type Failure = IpcError | string;

export function ipcError(code: IpcError["code"], detail: string | null = null): IpcError {
  return { code, message: `English fallback for ${code}`, detail };
}

// ---- Builders for contract data (no invented data reaches the app: tests pass it in) ----

export const IDLE: AccountStatus = { state: "idle", progress: null, error: null, retryAt: null, lastSyncAt: 1 };

export function makeAccount(overrides: Partial<Account> = {}): Account {
  const email = overrides.email ?? "ada@vicemail.test";
  const server = { host: "localhost", port: 1993, security: "tls" as const, username: email };
  return {
    id: "11111111-1111-4111-8111-111111111111",
    email,
    displayName: "Ada Lovelace",
    color: 0,
    imap: server,
    smtp: { ...server, port: 1465 },
    writesEnabled: false,
    status: IDLE,
    ...overrides,
  };
}

export function makeMailbox(overrides: Partial<Mailbox> & Pick<Mailbox, "id" | "path">): Mailbox {
  const segments = overrides.path.split("/");
  return {
    accountId: makeAccount().id,
    name: segments[segments.length - 1] ?? overrides.path,
    delimiter: "/",
    parentId: null,
    role: "custom",
    roleSource: "none",
    selectable: true,
    unread: 0,
    total: 0,
    ...overrides,
  };
}

/** Inbox, Sent, Drafts, Trash and two own folders of the default account. */
export function standardMailboxes(accountId = makeAccount().id): Mailbox[] {
  return [
    makeMailbox({ id: 1, accountId, path: "INBOX", role: "inbox", roleSource: "server", unread: 3, total: 10 }),
    makeMailbox({ id: 2, accountId, path: "Sent", role: "sent", roleSource: "server" }),
    makeMailbox({ id: 3, accountId, path: "Drafts", role: "drafts", roleSource: "heuristic" }),
    makeMailbox({ id: 4, accountId, path: "Trash", role: "trash", roleSource: "server" }),
    makeMailbox({ id: 5, accountId, path: "Projekte", unread: 2 }),
    makeMailbox({ id: 6, accountId, path: "Projekte/2026", parentId: 5, unread: 1 }),
  ];
}

let nextMessageId = 1;

export function makeMessage(overrides: Partial<MessageListItem> = {}): MessageListItem {
  const date = overrides.date ?? Date.now();
  return {
    id: overrides.id ?? Math.floor(date / 1000) * 1000 + (nextMessageId++ % 1000),
    accountId: makeAccount().id,
    mailboxId: 1,
    from: { name: "Grace Hopper", email: "grace@example.org" },
    to: [{ name: "Ada Lovelace", email: "ada@vicemail.test" }],
    subject: "Hello",
    date,
    unread: false,
    flagged: false,
    answered: false,
    draft: false,
    hasAttachments: false,
    keywords: [],
    ...overrides,
  };
}

/**
 * `count` messages of one folder, newest first, one minute apart starting at `newest`.
 * Ids follow the core's rule (sort date in seconds), so they are unique and descending.
 */
export function makeMessages(count: number, options: { mailboxId?: number; newest?: number } = {}): MessageListItem[] {
  const newest = options.newest ?? Date.now();
  return Array.from({ length: count }, (_, i) => {
    const date = newest - i * 60_000;
    return makeMessage({
      id: Math.floor(date / 1000),
      mailboxId: options.mailboxId ?? 1,
      date,
      subject: `Message ${String(i + 1)}`,
      from: { name: `Sender ${String(i + 1)}`, email: `sender${String(i + 1)}@example.org` },
      unread: i % 3 === 0,
    });
  });
}

export const CAPABILITIES: Capabilities = {
  imap4rev1: true,
  idle: true,
  condstore: true,
  qresync: true,
  enable: true,
  move: true,
  uidplus: true,
  specialUse: true,
  listStatus: true,
  literalPlus: true,
  esearch: true,
  names: ["IDLE", "IMAP4REV1"],
};

const NOTHING: DiscoveryResult = { source: "none", imap: null, smtp: null, providerHint: null };

interface FakeCoreOptions {
  appInfo?: AppInfo;
  preferences?: Preferences;
  failures?: Partial<Record<string, Failure>>;
  /** Fails a call by its arguments, e.g. only the second page of a list. */
  fail?: (cmd: string, args: Record<string, unknown>) => Failure | undefined;
  /** Commands whose answer never arrives (to see loading states). */
  hang?: readonly string[];
  /**
   * `remove_account` removes at once but answers only when this promise resolves. The real core
   * sends `accountsChanged` before it answers, so the list can refresh first.
   */
  holdRemoveAccount?: Promise<void>;
  /** Simulate the event plugin (listen/emit). Off: event calls reach `failures` like commands. */
  mockEvents?: boolean;
  accounts?: Account[];
  mailboxes?: Mailbox[];
  /** Messages by mailbox id, newest first. */
  messages?: Record<number, MessageListItem[]>;
  /** Discovery answers by domain. */
  knownProviders?: Record<string, DiscoveryResult>;
  online?: Record<string, DiscoveryResult>;
  capabilities?: Capabilities;
}

export interface FakeCore {
  readonly calls: { cmd: string; args: unknown }[];
  readonly preferences: Preferences;
  readonly accounts: Account[];
  readonly mailboxes: Mailbox[];
  /** Replaces the messages of a folder, as a sync would; the test then emits the event. */
  setMessages: (mailboxId: number, items: MessageListItem[]) => void;
  setMailboxes: (mailboxes: Mailbox[]) => void;
  /** Calls of one command, with their arguments. */
  callsOf: (cmd: string) => Record<string, unknown>[];
}

/**
 * Stands in for the Rust core behind `invoke`, with the command names and argument shapes of the
 * IPC contract. Events are mocked too, so `emit("core-event", …)` reaches `listen` handlers.
 */
export function mockCore(options: FakeCoreOptions = {}): FakeCore {
  const appInfo = options.appInfo ?? APP_INFO;
  let preferences: Preferences = options.preferences ?? { theme: "system", language: "system" };
  const calls: { cmd: string; args: unknown }[] = [];
  const accounts = [...(options.accounts ?? [])];
  let mailboxes = [...(options.mailboxes ?? [])];
  const messages = new Map<number, MessageListItem[]>(
    Object.entries(options.messages ?? {}).map(([id, items]) => [Number(id), items]),
  );

  const domain = (email: string) => email.split("@")[1]?.toLowerCase() ?? "";

  function listMessages(mailboxId: number, cursor: number | null, limit: number): MessagePage {
    const all = messages.get(mailboxId) ?? [];
    const start = cursor === null ? 0 : all.findIndex((m) => m.id < cursor);
    const items = start < 0 ? [] : all.slice(start, start + limit);
    const last = items[items.length - 1];
    const more = start >= 0 && start + limit < all.length;
    return { items, nextCursor: more && last ? last.id : null };
  }

  function setRole(mailboxId: number, role: MailboxRole): Mailbox[] | null {
    const target = mailboxes.find((m) => m.id === mailboxId);
    if (!target) return null;
    mailboxes = mailboxes.map((m) => {
      if (m.id === mailboxId) return { ...m, role, roleSource: "user" };
      if (role !== "custom" && m.accountId === target.accountId && m.role === role) {
        return { ...m, role: "custom", roleSource: "none" };
      }
      return m;
    });
    return mailboxes.filter((m) => m.accountId === target.accountId);
  }

  mockIPC(
    (cmd, rawArgs) => {
      calls.push({ cmd, args: rawArgs });
      const args = (rawArgs ?? {}) as Record<string, unknown>;
      const failure = options.failures?.[cmd] ?? options.fail?.(cmd, args);
      // Tauri rejects with the serialised error value (an object or a plain string), not an Error.
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
      if (failure !== undefined) return Promise.reject(failure);
      if (options.hang?.includes(cmd)) return new Promise(() => undefined);
      switch (cmd) {
        case "get_app_info":
          return appInfo;
        case "get_preferences":
          return preferences;
        case "set_preferences":
          preferences = (args as { preferences: Preferences }).preferences;
          return preferences;
        case "list_accounts":
          return accounts.map((a) => ({ ...a }));
        case "discover_known_provider":
          return options.knownProviders?.[domain(args["email"] as string)] ?? NOTHING;
        case "discover_online":
          return options.online?.[domain(args["email"] as string)] ?? NOTHING;
        case "test_connection":
          return { capabilities: options.capabilities ?? CAPABILITIES };
        case "add_account": {
          const settings = args["settings"] as AccountSettings;
          const account = makeAccount({
            id: "22222222-2222-4222-8222-222222222222",
            email: settings.email,
            displayName: settings.displayName,
            color: accounts.length % 8,
            imap: settings.imap,
            smtp: settings.smtp,
            writesEnabled: false,
            status: { state: "connecting", progress: null, error: null, retryAt: null, lastSyncAt: null },
          });
          accounts.push(account);
          return account;
        }
        case "remove_account": {
          const index = accounts.findIndex((a) => a.id === args["accountId"]);
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          if (index < 0) return Promise.reject(ipcError("not_found"));
          accounts.splice(index, 1);
          return options.holdRemoveAccount?.then(() => null) ?? null;
        }
        case "retry_account":
        case "update_password":
          return null;
        case "list_mailboxes":
          return mailboxes
            .filter((m) => m.accountId === args["accountId"])
            .sort((a, b) => a.path.localeCompare(b.path));
        case "set_mailbox_role": {
          const changed = setRole(args["mailboxId"] as number, args["role"] as MailboxRole);
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          return changed ?? Promise.reject(ipcError("not_found"));
        }
        case "list_messages":
          return listMessages(args["mailboxId"] as number, args["cursor"] as number | null, args["limit"] as number);
        default:
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          return Promise.reject(`command ${cmd} not found`);
      }
    },
    { shouldMockEvents: options.mockEvents ?? true },
  );

  return {
    calls,
    get preferences() {
      return preferences;
    },
    accounts,
    get mailboxes() {
      return mailboxes;
    },
    setMessages: (mailboxId, items) => {
      messages.set(mailboxId, items);
    },
    setMailboxes: (next) => {
      mailboxes = next;
    },
    callsOf: (cmd) => calls.filter((c) => c.cmd === cmd).map((c) => (c.args ?? {}) as Record<string, unknown>),
  };
}
