import { mockIPC } from "@tauri-apps/api/mocks";
import { isValidKeyword } from "../../src/features/actions/keywords";
import { createComposeFake, type ComposeFakeOptions, type ComposeFakeState } from "./compose";
import { createSearchFake, type SearchFakeOptions, type SearchFakeState } from "./search";
import { createMcpSyncFake, type McpSyncFakeOptions, type McpSyncFakeState } from "./mcpSync";
import { createComfortFake, type ComfortFake, type ComfortFakeOptions } from "./comfort";
import { createExtrasFake, type ExtrasFake, type ExtrasFakeOptions } from "./extras";
// Stage 10: signatures, templates and contacts.
import { createSnippetsFake, type SnippetsFakeOptions, type SnippetsFakeState } from "./snippets";
import { createContactsFake, type ContactsFakeOptions, type ContactsFakeState } from "./contacts";
// Stage 13: AI in the app.
import { createAiFake, type AiFakeOptions, type AiFakeState } from "./ai";
// Stage 8: UI preferences, window layouts, message and composer windows.
import { createUiFake, type UiFakeOptions, type UiFakeState } from "./uiPrefs";
import { createConversationFake, type ConversationFakeOptions } from "./conversations";
// Stage 9 and 13 gaps (decision 0027): `.eml` files and messages by id.
import { createEmlFake, type EmlFakeOptions, type EmlFakeState } from "./eml";
// Stage 16 (decision 0033): reading list, unread views, favourites, transfers, blocked senders.
import { createParityFake, type ParityFakeOptions, type ParityFakeState } from "./parity";
import { createParitySettingsFake, type ParitySettingsFakeOptions } from "./paritySettings";
// Stage 16: read receipts (RFC 8098).
import { createParityComposeFake, type ParityComposeFakeOptions, type ParityComposeFakeState } from "./parityCompose";
// Stage 16 (decision 0036): attachment previews, "Save all", pinned attachments, hidden blocked mail.
import { createAttachmentsFake, type AttachmentsFakeOptions, type AttachmentsFakeState } from "./attachments";
// Stage 15 (decision 0037): in-app updates.
import { createUpdatesFake, type UpdatesFakeOptions, type UpdatesFakeState } from "./updates";
// Stage 15 (decision 0044): the guided first start.
import { createFirstStartFake, type FirstStartFakeOptions, type FirstStartFakeState } from "./firstStart";
// Stage 14 (decision 0042): sign-in with Microsoft and Google.
import { createOAuthFake, type OAuthFakeOptions, type OAuthFakeState } from "./oauth";
import {
  MAX_ACTION_MESSAGES,
  type ActionKind,
  type ActionResult,
  type DiagnosticsExportResult,
  type MailAction,
  type MessageRef,
  type OperationInfo,
  type OperationKind,
  type OutboxCounts,
  type ReadingPreferences,
} from "../../src/services/ipc";
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
  LinkOpened,
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
  StartupProblem,
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
    oauth: null,
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
    category: null,
    important: false,
    pinned: false,
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
    // Stage 16 (decision 0036): no preview unless a test gives one.
    preview: null,
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
  gmail: false,
  names: ["IDLE", "IMAP4REV1"],
};

const NOTHING: DiscoveryResult = { source: "none", imap: null, smtp: null, providerHint: null };

interface FakeCoreOptions {
  appInfo?: AppInfo;
  /** What kept the database from opening at start (`getStartupProblem`); none by default. */
  startupProblem?: StartupProblem | null;
  /** Stage 14: Gmail labels by message id; messages without an entry are not Gmail (null). */
  gmailLabels?: Record<number, string[]>;
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
  /** Decision 0028: `openLink` answers by `<renderToken>/<index>`; default: opened in the system. */
  linkOpened?: Record<string, LinkOpened>;
  /** `saveAttachment` answers by `<messageId>/<index>`; default: saved under ~/Downloads. */
  saveResults?: Record<string, SaveAttachmentResult>;
  /** `openAttachment` answers by `<messageId>/<index>`; default: by the view's `executable`. */
  openResults?: Record<string, OpenAttachmentResult>;
  prefetchSettings?: PrefetchSettings;
  prefetchStatus?: Record<string, PrefetchStatus>;
  /** Senders allowed to load external content, by account id. */
  remoteSenders?: Record<string, string[]>;
  readingPreferences?: ReadingPreferences;
  /** Operations already in the outbox (activity view), newest first. */
  operations?: OperationInfo[];
  /** Stage 5: drafts, sending, outbox view, compose settings. */
  compose?: ComposeFakeOptions;
  /** Stage 7: search, conversations, unified inbox, groups, hidden accounts, identities. */
  search?: SearchFakeOptions;
  /** Stages 12 and 11: AI over MCP and the settings sync. */
  mcpSync?: McpSyncFakeOptions;
  /** Stage 9: sorted inbox, snooze, pins, tasks, sender rules, notification settings. */
  comfort?: ComfortFakeOptions;
  /** Stage 9 part B: unsubscribing, invitations, export, printing, default mail app, launch links. */
  extras?: ExtrasFakeOptions;
  /** Stage 10: signature pool, identity defaults and templates. */
  snippets?: SnippetsFakeOptions;
  /** Stage 10: the contact directory. */
  contacts?: ContactsFakeOptions;
  /** Stage 13: AI in the app (off unless a test turns it on) and the open-reply hints. */
  ai?: AiFakeOptions;
  /** Stage 8: UI preferences, the layout of this window and the windows of their own. */
  ui?: UiFakeOptions;
  /** Decision 0027: opened `.eml` files and the open dialog. */
  eml?: EmlFakeOptions;
  /** Stage 16: reading list, unread views, favourites and colours, transfers, blocked senders. */
  parity?: ParityFakeOptions;
  /** Stage 16: the notification sounds of this system (default: macOS). */
  paritySettings?: ParitySettingsFakeOptions;
  /** Stage 16: read receipts by message id. */
  receipts?: ParityComposeFakeOptions;
  /** Stage 16 (decision 0036): previews, "Save all", pinned attachments, hidden blocked mail. */
  attachments?: AttachmentsFakeOptions;
  /** Stage 15 (decision 0037): the update setting and state (default: a build without updater). */
  updates?: UpdatesFakeOptions;
  /** Stage 15: what "Export diagnostics…" answers (default: saved in Downloads). */
  diagnostics?: DiagnosticsExportResult;
  /** Decision 0039: which messages form a thread (default: every message is its own). */
  conversations?: ConversationFakeOptions;
  /** Stage 15 (decision 0044): the guided first start (default: ended, the frame shows). */
  firstStart?: FirstStartFakeOptions;
  /** Stage 14 (decision 0042): sign-in with Microsoft and Google (default: not in this build). */
  oauth?: OAuthFakeOptions;
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
  // ---- Stage 4: actions and the outbox ----
  /** Messages of a folder as the core holds them now (after actions). */
  messagesIn: (mailboxId: number) => MessageListItem[];
  /** The outbox, newest first. */
  readonly operations: OperationInfo[];
  /** The server's answer to queued operations: they end in `state` (the test then emits). */
  settle: (ids: readonly number[], state: OperationInfo["state"], extra?: Partial<OperationInfo>) => OperationInfo[];
  /** The server created the folders `createMailbox` asked for (the test then emits `mailboxesChanged`). */
  completeMailboxCreations: () => void;
  readonly readingPreferences: ReadingPreferences;
  /** Stage 5: drafts and sends as the fake core holds them now. */
  readonly compose: ComposeFakeState;
  /** Stage 7: groups, hidden accounts and identities as the fake core holds them now. */
  readonly search: SearchFakeState;
  /** Stages 12 and 11: MCP switch, shares, clients and the sync status as the fake holds them now. */
  readonly mcpSync: McpSyncFakeState;
  /** Stage 9: marks, settings and rules as the fake core holds them now. */
  readonly comfort: ComfortFake;
  /** Stage 9 part B: unsubscribe requests, opened invitations, exports, prints, default mail app. */
  readonly extras: ExtrasFake;
  /** Stage 10: signatures and templates as the fake core holds them now. */
  readonly snippets: SnippetsFakeState;
  /** Stage 10: contacts as the fake core holds them now. */
  readonly contacts: ContactsFakeState;
  /** Stage 13: AI settings, calls that would reach a provider, keys received, dismissed hints. */
  readonly ai: AiFakeState;
  /** Stage 8: UI preferences, layout and opened windows as the fake core holds them now. */
  readonly ui: UiFakeState;
  /** Decision 0027: dialogs shown, attachments saved or opened, `getMessagesByIds` calls. */
  readonly eml: EmlFakeState;
  /** Stage 16: reading list, unread views, transfers and blocked senders as the fake holds them now. */
  readonly parity: ParityFakeState;
  /** Stage 16: receipts the user sent or declined (only ever after a click). */
  readonly receipts: ParityComposeFakeState;
  /** Stage 16 (decision 0036): pins, preview pages asked for and "Save all" calls. */
  readonly attachments: AttachmentsFakeState;
  /** Stage 15 (decision 0037): the update setting and state as the fake holds them now. */
  readonly updates: UpdatesFakeState;
  /** Stage 15 (decision 0044): the first start as the fake holds it now. */
  readonly firstStart: FirstStartFakeState;
  /** Stage 14 (decision 0042): sign-ins the UI started. */
  readonly oauth: OAuthFakeState;
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

  // ---- Stage 4: a small outbox with the contract's rules ----
  let readingPreferences: ReadingPreferences = options.readingPreferences ?? { markReadOnOpen: "afterDelay" };
  interface Recorded {
    info: OperationInfo;
    /** Takes the local change back (undo, failure). */
    revert: () => void;
  }
  let operations: Recorded[] = (options.operations ?? []).map((info) => ({ info, revert: () => undefined }));
  let nextOperation = Math.max(0, ...operations.map((o) => o.info.id)) + 1;
  let nextBatch = Math.max(0, ...operations.map((o) => o.info.batch)) + 1;
  const requests = new Map<string, ActionResult>();
  const creations: { accountId: string; path: string; role: MailboxRole | null }[] = [];

  const reject = (code: IpcError["code"], detail: string | null = null) =>
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    Promise.reject(ipcError(code, detail));

  const accountOf = (id: string) => accounts.find((a) => a.id === id);
  const mailboxOf = (id: number) => mailboxes.find((m) => m.id === id);
  const roleMailbox = (accountId: string, role: MailboxRole) =>
    mailboxes.find((m) => m.accountId === accountId && m.role === role);

  const replaceItem = (mailboxId: number, id: number, change: (item: MessageListItem) => MessageListItem) => {
    messages.set(
      mailboxId,
      (messages.get(mailboxId) ?? []).map((item) => (item.id === id ? change(item) : item)),
    );
  };
  const moveItem = (from: number, to: number, id: number) => {
    const item = (messages.get(from) ?? []).find((m) => m.id === id);
    if (!item) return;
    messages.set(from, (messages.get(from) ?? []).filter((m) => m.id !== id));
    messages.set(to, [...(messages.get(to) ?? []), { ...item, mailboxId: to }].sort((a, b) => b.id - a.id));
  };

  const record = (
    batch: number,
    accountId: string,
    action: ActionKind,
    kind: OperationKind,
    ref: { mailboxId: number | null; messageId: number | null; target?: number | null },
    revert: () => void,
  ) => {
    const now = Date.now();
    const info: OperationInfo = {
      id: nextOperation++,
      batch,
      accountId,
      action,
      kind,
      state: "queued",
      mailboxId: ref.mailboxId,
      messageId: ref.messageId,
      targetMailboxId: ref.target ?? null,
      attempts: 0,
      error: null,
      conflict: null,
      notice: null,
      createdAt: now,
      updatedAt: now,
    };
    operations = [{ info, revert }, ...operations];
  };

  const ACTION_KINDS: Record<MailAction["type"], ActionKind> = {
    markRead: "mark_read",
    markUnread: "mark_unread",
    flag: "flag",
    unflag: "unflag",
    addKeyword: "add_keyword",
    removeKeyword: "remove_keyword",
    addLabel: "add_label",
    removeLabel: "remove_label",
    archive: "archive",
    delete: "delete",
    deletePermanently: "delete_permanently",
    move: "move",
    spam: "spam",
    notSpam: "not_spam",
  };

  function applyAction(action: MailAction, refs: MessageRef[], requestId: string): ActionResult | Promise<never> {
    if (!/^[A-Za-z0-9_-]{1,64}$/.test(requestId)) return reject("invalid_argument", "request id");
    if (refs.length === 0 || refs.length > MAX_ACTION_MESSAGES) return reject("invalid_argument", "messages");
    const accountIds = new Set(refs.map((ref) => mailboxOf(ref.mailboxId)?.accountId));
    const [accountId] = [...accountIds];
    if (accountIds.size !== 1 || !accountId) return reject("invalid_argument", "accounts");
    const account = accountOf(accountId);
    if (!account) return reject("not_found");
    if (!account.writesEnabled) return reject("read_only");
    const earlier = requests.get(requestId);
    if (earlier) return { batch: earlier.batch, queued: 0, cancelled: 0 };
    const found = refs.map((ref) => (messages.get(ref.mailboxId) ?? []).find((m) => m.id === ref.messageId));
    if (found.some((item) => !item)) return reject("not_found");
    const items = found as MessageListItem[];
    const batch = nextBatch;
    let queued = 0;
    const kind = ACTION_KINDS[action.type];

    const setFlag = (item: MessageListItem, field: "unread" | "flagged", value: boolean) => {
      if (item[field] === value) return;
      replaceItem(item.mailboxId, item.id, (m) => ({ ...m, [field]: value }));
      record(batch, accountId, kind, "set_flag", { mailboxId: item.mailboxId, messageId: item.id }, () => {
        replaceItem(item.mailboxId, item.id, (m) => ({ ...m, [field]: !value }));
      });
      queued += 1;
    };
    const moveTo = (item: MessageListItem, target: number, opKind: OperationKind = "move") => {
      if (item.mailboxId === target) return;
      moveItem(item.mailboxId, target, item.id);
      record(batch, accountId, kind, opKind, { mailboxId: item.mailboxId, messageId: item.id, target }, () => {
        moveItem(target, item.mailboxId, item.id);
      });
      queued += 1;
    };
    const roleTarget = (role: MailboxRole) => roleMailbox(accountId, role);

    switch (action.type) {
      case "markRead":
      case "markUnread":
        for (const item of items) setFlag(item, "unread", action.type === "markUnread");
        break;
      case "flag":
      case "unflag":
        for (const item of items) setFlag(item, "flagged", action.type === "flag");
        break;
      case "addKeyword":
      case "removeKeyword": {
        if (!isValidKeyword(action.keyword)) return reject("invalid_keyword");
        const add = action.type === "addKeyword";
        for (const item of items) {
          const has = item.keywords.includes(action.keyword);
          if (has === add) continue;
          const change = (m: MessageListItem, on: boolean) => ({
            ...m,
            keywords: on ? [...m.keywords, action.keyword] : m.keywords.filter((k) => k !== action.keyword),
          });
          replaceItem(item.mailboxId, item.id, (m) => change(m, add));
          record(batch, accountId, kind, "set_flag", { mailboxId: item.mailboxId, messageId: item.id }, () => {
            replaceItem(item.mailboxId, item.id, (m) => change(m, !add));
          });
          queued += 1;
        }
        break;
      }
      case "archive":
      case "spam": {
        const target = roleTarget(action.type === "archive" ? "archive" : "junk");
        if (!target) return reject("missing_mailbox");
        for (const item of items) moveTo(item, target.id);
        break;
      }
      case "delete": {
        const trash = roleTarget("trash");
        if (!trash) return reject("missing_mailbox");
        if (items.some((item) => item.mailboxId === trash.id)) return reject("confirmation_required");
        for (const item of items) moveTo(item, trash.id);
        break;
      }
      case "notSpam": {
        const junk = roleTarget("junk");
        const inbox = roleTarget("inbox");
        if (!inbox) return reject("missing_mailbox");
        for (const item of items) if (item.mailboxId === junk?.id) moveTo(item, inbox.id);
        break;
      }
      case "move": {
        const target = mailboxOf(action.target);
        if (!target || target.accountId !== accountId || !target.selectable) return reject("invalid_argument", "target");
        for (const item of items) moveTo(item, target.id);
        break;
      }
      case "deletePermanently":
        for (const item of items) {
          messages.set(item.mailboxId, (messages.get(item.mailboxId) ?? []).filter((m) => m.id !== item.id));
          record(batch, accountId, kind, "expunge", { mailboxId: item.mailboxId, messageId: item.id }, () => undefined);
          queued += 1;
        }
        break;
    }
    const result: ActionResult = { batch: queued > 0 ? batch : null, queued, cancelled: 0 };
    if (queued > 0) nextBatch += 1;
    requests.set(requestId, result);
    return result;
  }

  function undoAction(batch: number): ActionResult | Promise<never> {
    const ops = operations.filter((o) => o.info.batch === batch);
    if (ops.length === 0) return reject("not_found");
    if (ops.some((o) => o.info.kind === "expunge" || o.info.kind === "create_mailbox")) return reject("not_undoable");
    for (const op of ops) op.revert();
    operations = operations.filter((o) => o.info.batch !== batch);
    return { batch: null, queued: 0, cancelled: ops.length };
  }

  function outboxCounts(accountId: string): OutboxCounts {
    const of = operations.filter((o) => o.info.accountId === accountId).map((o) => o.info.state);
    return {
      accountId,
      pending: of.filter((state) => state === "queued" || state === "in_flight").length,
      failed: of.filter((state) => state === "failed").length,
      conflict: of.filter((state) => state === "conflict").length,
    };
  }

  function handleOutbox(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "get_reading_preferences":
        return readingPreferences;
      case "set_reading_preferences":
        readingPreferences = args["preferences"] as ReadingPreferences;
        return readingPreferences;
      case "apply_action":
        return applyAction(args["action"] as MailAction, args["messages"] as MessageRef[], args["requestId"] as string);
      // Stage 14: Gmail labels, kept per message in the options' map.
      case "get_message_labels":
        return options.gmailLabels?.[args["messageId"] as number] ?? null;
      case "set_message_label": {
        const labels = options.gmailLabels?.[args["messageId"] as number];
        if (!labels) return reject("unsupported_server");
        const label = args["label"] as string;
        const next = labels.filter((l) => l !== label);
        if (args["add"] as boolean) next.push(label);
        (options.gmailLabels ?? {})[args["messageId"] as number] = next;
        return { batch: nextBatch++, queued: 1, cancelled: 0 };
      }
      case "undo_action":
        return undoAction(args["batch"] as number);
      case "create_mailbox": {
        const account = accountOf(args["accountId"] as string);
        if (!account) return reject("not_found");
        if (!account.writesEnabled) return reject("read_only");
        const batch = nextBatch++;
        record(batch, account.id, "create_mailbox", "create_mailbox", { mailboxId: null, messageId: null }, () => undefined);
        creations.push({ accountId: account.id, path: args["path"] as string, role: args["role"] as MailboxRole | null });
        return { batch, queued: 1, cancelled: 0 };
      }
      case "list_operations": {
        const before = args["before"] as number | null;
        return operations
          .map((o) => o.info)
          .filter((o) => o.accountId === args["accountId"] && (before === null || o.id < before))
          .slice(0, args["limit"] as number);
      }
      case "retry_operation": {
        const op = operations.find((o) => o.info.id === args["operationId"]);
        if (!op) return reject("not_found");
        if (op.info.state !== "failed" && op.info.state !== "conflict") return reject("invalid_argument");
        const now = Date.now();
        const fresh = { ...op.info, id: nextOperation++, state: "queued" as const, error: null, conflict: null, updatedAt: now };
        operations = [{ info: fresh, revert: op.revert }, ...operations.filter((o) => o !== op)];
        return { batch: op.info.batch, queued: 1, cancelled: 0 };
      }
      case "discard_operation": {
        const op = operations.find((o) => o.info.id === args["operationId"]);
        if (!op) return reject("not_found");
        if (op.info.state === "queued" || op.info.state === "in_flight") return reject("invalid_argument");
        operations = operations.filter((o) => o !== op);
        return null;
      }
      case "get_outbox_counts":
        return outboxCounts(args["accountId"] as string);
      case "set_writes_enabled": {
        const index = accounts.findIndex((a) => a.id === args["accountId"]);
        const account = accounts[index];
        if (!account) return reject("not_found");
        const next = { ...account, writesEnabled: args["enabled"] as boolean };
        accounts[index] = next;
        return { ...next };
      }
      default:
        return undefined;
    }
  }

  const searchFake = createSearchFake(options.search ?? {}, {
    accounts: () => accounts,
    mailboxes: () => mailboxes,
    messages: (mailboxId) => messages.get(mailboxId) ?? [],
  });

  // Stages 12 and 11: AI over MCP and the settings sync.
  const mcpSyncFake = createMcpSyncFake(options.mcpSync ?? {}, { accounts: () => accounts, calls: () => calls });

  // Stage 5: drafts, sending, outbox view, compose settings.
  const composeFake = createComposeFake(options.compose ?? {}, { accounts: () => accounts });

  // Stage 9: sorted inbox, snooze, pins, tasks, sender rules, notification settings.
  const comfortFake = createComfortFake(options.comfort ?? {}, {
    messagesIn: (mailboxId) => messages.get(mailboxId) ?? [],
    listMessages,
    reject: (code) => reject(code),
    roleOf: (mailboxId) => mailboxes.find((mailbox) => mailbox.id === mailboxId)?.role,
  });

  // Stage 9 part B: unsubscribing, invitations, export, printing, default mail app, mailto.
  const extrasFake = createExtrasFake(options.extras ?? {});

  // Stage 10: signatures, templates and contacts.
  const snippetsFake = createSnippetsFake(options.snippets ?? {}, { drafts: () => composeFake.state.drafts });
  const contactsFake = createContactsFake(options.contacts ?? {});

  // Stage 13: AI in the app.
  const aiFake = createAiFake(options.ai ?? {}, {
    accounts: () => accounts,
    accountOf: (messageId) => [...messages.values()].flat().find((m) => m.id === messageId)?.accountId ?? null,
    createReply: (messageId) => composeFake.handle("create_draft", { request: { kind: "reply", messageId, language: "en" } }),
  });

  // Stage 8: UI preferences, window layouts, message and composer windows.
  // Decision 0027: `.eml` files and messages by id.
  const emlFake = createEmlFake(options.eml ?? {}, {
    messages: () => [...messages.values()].flat(),
    reject: (code, detail = null) => reject(code, detail),
  });

  const uiFake = createUiFake(options.ui ?? {}, {
    accountOfMessage: (messageId) => [...messages.values()].flat().find((m) => m.id === messageId)?.accountId ?? null,
    hasDraft: (draftId) => composeFake.state.drafts.has(draftId),
    reject: (code, detail = null) => reject(code, detail),
  });

  // Stage 16: notification sounds of this system.
  const paritySettingsFake = createParitySettingsFake(options.paritySettings ?? {});
  // Stage 16: reading list, unread views, favourites and colours, transfers, blocked senders.
  // Stage 16: read receipts; the policy is the compose setting.
  const parityComposeFake = createParityComposeFake(options.receipts ?? {}, {
    policy: () => composeFake.state.settings.readReceipts ?? "ask",
    accountOf: (messageId) => {
      const accountId = [...messages.values()].flat().find((m) => m.id === messageId)?.accountId;
      const account = accounts.find((a) => a.id === accountId);
      return account ? { id: account.id, writesEnabled: account.writesEnabled } : null;
    },
    reject: (code, detail = null) => reject(code, detail),
  });

  const parityFake = createParityFake(options.parity ?? {}, {
    messages: () => [...messages.values()].flat(),
    mailboxes: () => mailboxes,
    setMailboxes: (next) => {
      mailboxes = next;
    },
    writesEnabled: (accountId) => accounts.find((a) => a.id === accountId)?.writesEnabled ?? false,
    reject: (code, detail = null) => reject(code, detail),
  });

  const attachmentsFake = createAttachmentsFake(options.attachments ?? {}, {
    messages: () => [...messages.values()].flat(),
    attachmentsOf: (messageId) => viewOf(messageId, "auto")?.attachments ?? [],
    reject: (code, detail = null) => reject(code, detail),
  });

  const updatesFake = createUpdatesFake(options.updates ?? {}, { reject: (code) => reject(code) });
  // Stage 15 (decision 0044): the guided first start.
  const firstStartFake = createFirstStartFake(options.firstStart ?? {}, { accounts: () => accounts });
  // Stage 14 (decision 0042): the account appears read-only, like one added with a password.
  const oauthFake = createOAuthFake(options.oauth ?? {}, {
    add: (settings, provider) => {
      const account = makeAccount({
        id: "44444444-4444-4444-8444-444444444444",
        email: settings.email,
        displayName: settings.displayName,
        color: accounts.length % 8,
        imap: settings.imap,
        smtp: settings.smtp,
        oauth: provider,
        writesEnabled: false,
        status: { state: "connecting", progress: null, error: null, retryAt: null, lastSyncAt: null },
      });
      accounts.push(account);
      return account;
    },
    signedIn: (accountId) => {
      const account = accounts.find((a) => a.id === accountId);
      if (account) account.status = IDLE;
    },
  });

  // Stage 16: rows carry `readingList` wherever the fake core lists them (as the core does).
  const markReadingList = (answer: unknown): unknown => {
    const ids = parityFake.state.readingList;
    if (ids.length === 0 || answer === null || typeof answer !== "object" || answer instanceof Promise) return answer;
    const walk = (value: unknown): unknown => {
      if (Array.isArray(value)) return value.map(walk);
      if (value === null || typeof value !== "object") return value;
      const record = value as Record<string, unknown>;
      const next: Record<string, unknown> = {};
      for (const [key, child] of Object.entries(record)) next[key] = walk(child);
      const isRow = typeof record["id"] === "number" && typeof record["subject"] === "string" && "mailboxId" in record;
      if (isRow && ids.includes(record["id"] as number)) next["readingList"] = true;
      return next;
    };
    return walk(answer);
  };

  mockIPC(
    (cmd, rawArgs) => {
      const answer = handleCommand(cmd, rawArgs);
      return cmd.startsWith("list_") || cmd === "get_thread" ? markReadingList(answer) : answer;
    },
    { shouldMockEvents: options.mockEvents ?? true },
  );

  // Decision 0039: conversation lists group the message lists of the fakes above.
  const conversationFake = createConversationFake(options.conversations ?? {}, {
    messages: () => [...messages.values()].flat(),
    answer: (cmd, args) => handleCommand(cmd, args, false),
  });

  /** `record`: false for a fake asking another one (decision 0039), not a call of the UI. */
  function handleCommand(cmd: string, rawArgs: unknown, record = true): unknown {
    {
      const args = (rawArgs ?? {}) as Record<string, unknown>;
      if (record) {
        calls.push({ cmd, args: rawArgs });
        const failure = options.failures?.[cmd] ?? options.fail?.(cmd, args);
        // Tauri rejects with the serialised error value (an object or a plain string), not an Error.
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (failure !== undefined) return Promise.reject(failure);
        if (options.hang?.includes(cmd)) return new Promise(() => undefined);
        const conversationAnswer = conversationFake.handle(cmd, args);
        if (conversationAnswer !== undefined) return conversationAnswer;
      }
      switch (cmd) {
        case "get_app_info":
          return appInfo;
        case "get_startup_problem":
          return options.startupProblem ?? null;
        // Stage 15: the core shows the save dialog and writes the file.
        case "export_diagnostics":
          return options.diagnostics ?? { kind: "saved", path: "/Users/test/Downloads/app-diagnostics-2026-11-23.txt" };
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
            // Stage 14 (decision 0041): a JMAP account says so.
            ...(settings.protocol === "jmap" ? { protocol: "jmap" as const } : {}),
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
          return options.linkOpened?.[`${String(args["renderToken"])}/${String(args["index"])}`] ?? { kind: "opened" };
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
        default: {
          // Stage 5 first: it answers `retry_operation` for sends only.
          const composeAnswer = composeFake.handle(cmd, args);
          if (composeAnswer !== undefined) return composeAnswer;
          const answer = handleOutbox(cmd, args);
          if (answer !== undefined) return answer;
          // Stage 7: search, conversations, unified inbox, groups, identities.
          const searchAnswer = searchFake.handle(cmd, args);
          if (searchAnswer !== undefined) return searchAnswer;
          // Stages 12 and 11: AI over MCP and the settings sync.
          const mcpSyncAnswer = mcpSyncFake.handle(cmd, args);
          if (mcpSyncAnswer !== undefined) return mcpSyncAnswer;
          // Stage 16 before stage 9: it answers `listInboxFiltered` with the unread filter.
          const parityAnswer = parityFake.handle(cmd, args);
          if (parityAnswer !== undefined) return parityAnswer;
          // Stage 16 (decision 0036): previews, "Save all", pinned attachments, hidden blocked mail.
          const attachmentsAnswer = attachmentsFake.handle(cmd, args);
          if (attachmentsAnswer !== undefined) return attachmentsAnswer;
          const paritySettingsAnswer = paritySettingsFake.handle(cmd);
          if (paritySettingsAnswer !== undefined) return paritySettingsAnswer;
          const parityComposeAnswer = parityComposeFake.handle(cmd, args);
          if (parityComposeAnswer !== undefined) return parityComposeAnswer;
          // Stage 9: sorted inbox, snooze, pins, tasks, sender rules, notification settings.
          const comfortAnswer = comfortFake.handle(cmd, args);
          if (comfortAnswer !== undefined) return comfortAnswer;
          // Stage 9 part B: unsubscribing, invitations, export, printing, default mail app, mailto.
          const extrasAnswer = extrasFake.handle(cmd, args);
          if (extrasAnswer !== undefined) return extrasAnswer;
          // Stage 10: signatures, templates and contacts.
          const snippetsAnswer = snippetsFake.handle(cmd, args);
          if (snippetsAnswer !== undefined) return snippetsAnswer;
          const contactsAnswer = contactsFake.handle(cmd, args);
          if (contactsAnswer !== undefined) return contactsAnswer;
          // Stage 13: AI in the app.
          const aiAnswer = aiFake.handle(cmd, args);
          if (aiAnswer !== undefined) return aiAnswer;
          // Stage 8: UI preferences, window layouts, message and composer windows.
          const uiAnswer = uiFake.handle(cmd, args);
          if (uiAnswer !== undefined) return uiAnswer;
          // Decision 0027: `.eml` files and messages by id.
          const emlAnswer = emlFake.handle(cmd, args);
          if (emlAnswer !== undefined) return emlAnswer;
          // Stage 15 (decision 0037): in-app updates.
          const updatesAnswer = updatesFake.handle(cmd, args);
          if (updatesAnswer !== undefined) return updatesAnswer;
          // Stage 15 (decision 0044): the guided first start.
          const firstStartAnswer = firstStartFake.handle(cmd, args);
          if (firstStartAnswer !== undefined) return firstStartAnswer;
          // Stage 14 (decision 0042): sign-in with Microsoft and Google.
          const oauthAnswer = oauthFake.handle(cmd, args);
          if (oauthAnswer !== undefined) return oauthAnswer;
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          return Promise.reject(`command ${cmd} not found`);
        }
      }
    }
  }

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
    messagesIn: (mailboxId) => [...(messages.get(mailboxId) ?? [])],
    get operations() {
      return operations.map((o) => o.info);
    },
    settle: (ids, state, extra = {}) => {
      const changed: OperationInfo[] = [];
      operations = operations.map((o) => {
        if (!ids.includes(o.info.id)) return o;
        // A failure takes the local change back, as the core does.
        if (state === "failed") o.revert();
        const info = { ...o.info, state, ...extra, updatedAt: Date.now() };
        changed.push(info);
        return { ...o, info };
      });
      return changed;
    },
    completeMailboxCreations: () => {
      for (const creation of creations.splice(0)) {
        const id = Math.max(0, ...mailboxes.map((m) => m.id)) + 1;
        mailboxes = [
          ...mailboxes,
          makeMailbox({
            id,
            accountId: creation.accountId,
            path: creation.path,
            role: creation.role ?? "custom",
            roleSource: creation.role ? "user" : "none",
          }),
        ];
      }
    },
    get readingPreferences() {
      return readingPreferences;
    },
    compose: composeFake.state,
    search: searchFake.state,
    mcpSync: mcpSyncFake.state,
    comfort: comfortFake,
    extras: extrasFake,
    snippets: snippetsFake.state,
    contacts: contactsFake.state,
    ai: aiFake.state,
    ui: uiFake.state,
    eml: emlFake.state,
    parity: parityFake.state,
    receipts: parityComposeFake.state,
    attachments: attachmentsFake.state,
    updates: updatesFake.state,
    firstStart: firstStartFake.state,
    oauth: oauthFake.state,
  };
}
