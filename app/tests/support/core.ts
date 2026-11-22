import { mockIPC } from "@tauri-apps/api/mocks";
import type {
  Account,
  AccountSettings,
  AccountStatus,
  AppInfo,
  AttachmentInfo,
  BodyFormat,
  Capabilities,
  DiscoveryResult,
  IpcError,
  LinkInfo,
  Mailbox,
  MailboxRole,
  MessageBody,
  MessageListItem,
  MessagePage,
  MessageView,
  OpenAttachmentResult,
  Preferences,
  PrefetchSettings,
  PrefetchStatus,
  RemoteContentScope,
  SaveAttachmentResult,
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
    preview: null,
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

// ---- Stage 3: the reading pane ----

/** The mandatory meta CSP every `srcdoc` from the core starts with (decision 0005). */
export const MAIL_CSP =
  "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data:; style-src 'unsafe-inline'; font-src data:; base-uri 'none'; form-action 'none'\">";

let nextToken = 1;

/** A fresh render token (32 hex digits), as the core makes one per rendering. */
export function renderToken(): string {
  return (nextToken++).toString(16).padStart(32, "0");
}

export function htmlBody(html: string, overrides: Partial<Extract<MessageBody, { kind: "html" }>> = {}): MessageBody {
  return { kind: "html", srcdoc: MAIL_CSP + html, renderToken: renderToken(), blockedRemote: 0, remoteLoaded: false, ...overrides };
}

export function textBody(segments: Extract<MessageBody, { kind: "text" }>["segments"], token = renderToken()): MessageBody {
  return { kind: "text", segments, renderToken: token };
}

export function makeAttachment(overrides: Partial<AttachmentInfo> = {}): AttachmentInfo {
  return {
    index: 0,
    name: "report.pdf",
    mimeType: "application/pdf",
    size: 1_258_291,
    executable: false,
    local: true,
    ...overrides,
  };
}

/** What `getMessage` answers for a message of the list (same id, account, sender, subject). */
export function makeView(item: MessageListItem, overrides: Partial<MessageView> = {}): MessageView {
  return {
    id: item.id,
    accountId: item.accountId,
    header: {
      from: item.from ? [item.from] : [],
      to: item.to,
      cc: [],
      bcc: [],
      replyTo: [],
      date: item.date,
      subject: item.subject,
    },
    body: htmlBody("<p>Hello from the core</p>"),
    formats: ["html", "text"],
    attachments: [],
    partial: false,
    problems: [],
    ...overrides,
  };
}

export const PREFETCH_DEFAULTS: PrefetchSettings = { period: "days30", maxMessageSize: 5 * 1024 * 1024 };
const PREFETCH_IDLE: PrefetchStatus = { state: "idle", done: 0, total: 0, bytes: 0 };
const MAX_PREFETCH_SIZE = 256 * 1024 * 1024;

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
  /** `getMessage` answers by message id: one view, or one per asked format. Unknown: `not_found`. */
  views?: Record<number, MessageView | ((format: BodyFormat) => MessageView)>;
  /** `loadRemoteContent` answers by message id. */
  remoteViews?: Record<number, MessageView>;
  /** `describeLink` answers by `<renderToken>/<index>`. Unknown: `not_found` (a forgotten rendering). */
  links?: Record<string, LinkInfo>;
  /** `saveAttachment` answers by `<messageId>/<index>`; default: saved under ~/Downloads. */
  saveResults?: Record<string, SaveAttachmentResult>;
  /** `openAttachment` answers by `<messageId>/<index>`; default: by the view's `executable`. */
  openResults?: Record<string, OpenAttachmentResult>;
  prefetchSettings?: PrefetchSettings;
  prefetchStatus?: Record<string, PrefetchStatus>;
  /** Senders allowed to load external content, by account id. */
  remoteSenders?: Record<string, string[]>;
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
  /** Replaces what `getMessage` answers for a message (a fetch finished); the test then emits. */
  setView: (messageId: number, view: MessageView | ((format: BodyFormat) => MessageView)) => void;
  remoteSenders: (accountId: string) => string[];
  readonly prefetchSettings: PrefetchSettings;
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

  const views = new Map(Object.entries(options.views ?? {}).map(([id, view]) => [Number(id), view]));
  const senders = new Map(Object.entries(options.remoteSenders ?? {}).map(([id, list]) => [id, [...list]]));
  let prefetchSettings: PrefetchSettings = options.prefetchSettings ?? PREFETCH_DEFAULTS;

  function viewOf(messageId: number, format: BodyFormat): MessageView | undefined {
    const view = views.get(messageId);
    return typeof view === "function" ? view(format) : view;
  }

  function openAttachment(messageId: number, index: number, confirmed: boolean): OpenAttachmentResult | undefined {
    const fixed = options.openResults?.[`${String(messageId)}/${String(index)}`];
    if (fixed && !confirmed) return fixed;
    const attachment = viewOf(messageId, "auto")?.attachments.find((a) => a.index === index);
    if (!attachment) return undefined;
    return attachment.executable && !confirmed ? { kind: "needsConfirmation" } : { kind: "opened" };
  }

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
        case "get_message": {
          const view = viewOf(args["messageId"] as number, args["format"] as BodyFormat);
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          return view ?? Promise.reject(ipcError("not_found"));
        }
        case "load_remote_content": {
          const messageId = args["messageId"] as number;
          const view = options.remoteViews?.[messageId];
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          if (!view) return Promise.reject(ipcError("not_found"));
          const sender = view.header.from[0]?.email.toLowerCase();
          if ((args["scope"] as RemoteContentScope) === "sender" && sender) {
            const list = senders.get(view.accountId) ?? [];
            if (!list.includes(sender)) list.push(sender);
            senders.set(view.accountId, list);
          }
          return view;
        }
        case "list_remote_content_senders":
          return [...(senders.get(args["accountId"] as string) ?? [])];
        case "revoke_remote_content": {
          const accountId = args["accountId"] as string;
          senders.set(accountId, (senders.get(accountId) ?? []).filter((s) => s !== args["sender"]));
          return null;
        }
        case "describe_link": {
          const link = options.links?.[`${String(args["renderToken"])}/${String(args["index"])}`];
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          return link ?? Promise.reject(ipcError("not_found"));
        }
        case "open_link":
          return null;
        case "save_attachment":
          return (
            options.saveResults?.[`${String(args["messageId"])}/${String(args["index"])}`] ?? {
              kind: "saved",
              path: "/Users/ada/Downloads/report.pdf",
            }
          );
        case "open_attachment": {
          const result = openAttachment(
            args["messageId"] as number,
            args["index"] as number,
            args["confirmedExecutable"] as boolean,
          );
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          return result ?? Promise.reject(ipcError("not_found"));
        }
        case "get_prefetch_settings":
          return prefetchSettings;
        case "set_prefetch_settings": {
          const next = args["settings"] as PrefetchSettings;
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          if (next.maxMessageSize > MAX_PREFETCH_SIZE) return Promise.reject(ipcError("invalid_argument"));
          prefetchSettings = next;
          return prefetchSettings;
        }
        case "get_prefetch_status":
          return options.prefetchStatus?.[args["accountId"] as string] ?? PREFETCH_IDLE;
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
    setView: (messageId, view) => {
      views.set(messageId, view);
    },
    remoteSenders: (accountId) => [...(senders.get(accountId) ?? [])],
    get prefetchSettings() {
      return prefetchSettings;
    },
  };
}
