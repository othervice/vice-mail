// The only module that talks to the core. It is also the only module allowed to import the
// generated bindings and `@tauri-apps/api` (enforced by ESLint `no-restricted-imports`).
//
// Command names, argument names, result types and the event name come from the bindings that
// tauri-specta generates from Rust, so a rename on the Rust side breaks the TypeScript build.
import {
  commands,
  events,
  MARK_READ_DELAY_MS,
  MAX_ACTION_MESSAGES,
  type Account,
  type AccountColor,
  type AccountId,
  type AccountSettings,
  type AccountState,
  type AccountStatus,
  type ActionKind,
  type ActionResult,
  type Address,
  type AppInfo,
  type AttachmentInfo,
  type BatchId,
  type BodyFormat,
  type BodyUnavailableReason,
  type Capabilities,
  type ConflictReason,
  type ConnectionTestResult,
  type CoreEvent,
  type DiscoveryResult,
  type DiscoverySource,
  type ErrorCode,
  type IpcError,
  type LanguagePreference,
  type LinkInfo,
  type LinkOpened,
  type LinkWarning,
  type ListCursor,
  type MailAction,
  type Mailbox,
  type MailboxId,
  type MailboxRole,
  type MarkReadOnOpen,
  type MessageActivity,
  type MessageBody,
  type MessageId,
  type MessageListItem,
  type MessagePage,
  type MessageProblem,
  type MessageRef,
  type MessageView,
  type MessageViewHeader,
  type OpenAttachmentResult,
  type OperationId,
  type OperationInfo,
  type OperationKind,
  type OperationNotice,
  type OperationState,
  type OutboxCounts,
  type Platform,
  type Preferences,
  type PrefetchPeriod,
  type PrefetchSettings,
  type PrefetchState,
  type PrefetchStatus,
  type ProviderHint,
  type ReadingPreferences,
  type RemoteContentScope,
  type RenderToken,
  type RoleSource,
  type SaveAttachmentResult,
  type Security,
  type ServerSettings,
  type SyncProgress,
  type TextSegment,
  type ThemePreference,
} from "../generated/bindings";

export { MARK_READ_DELAY_MS, MAX_ACTION_MESSAGES };

export type {
  Account,
  AccountColor,
  AccountId,
  AccountSettings,
  AccountState,
  AccountStatus,
  ActionKind,
  ActionResult,
  Address,
  AppInfo,
  AttachmentInfo,
  BatchId,
  BodyFormat,
  BodyUnavailableReason,
  Capabilities,
  ConflictReason,
  ConnectionTestResult,
  CoreEvent,
  DiscoveryResult,
  DiscoverySource,
  ErrorCode,
  IpcError,
  LanguagePreference,
  LinkInfo,
  LinkOpened,
  LinkWarning,
  ListCursor,
  MailAction,
  Mailbox,
  MailboxId,
  MailboxRole,
  MarkReadOnOpen,
  MessageActivity,
  MessageBody,
  MessageId,
  MessageListItem,
  MessagePage,
  MessageProblem,
  MessageRef,
  MessageView,
  MessageViewHeader,
  OpenAttachmentResult,
  OperationId,
  OperationInfo,
  OperationKind,
  OperationNotice,
  OperationState,
  OutboxCounts,
  Platform,
  Preferences,
  PrefetchPeriod,
  PrefetchSettings,
  PrefetchState,
  PrefetchStatus,
  ProviderHint,
  ReadingPreferences,
  RemoteContentScope,
  RenderToken,
  RoleSource,
  SaveAttachmentResult,
  Security,
  ServerSettings,
  SyncProgress,
  TextSegment,
  ThemePreference,
};

/** Every error code the core can send. `satisfies` plus the check below keep it complete. */
export const ERROR_CODES = [
  "internal",
  "invalid_argument",
  "not_found",
  "database",
  "database_too_new",
  "auth_failed",
  "cert_untrusted",
  "cert_name_mismatch",
  "cert_expired",
  "tls_failed",
  "starttls_unavailable",
  "connection_refused",
  "connection_timeout",
  "connection_lost",
  "dns_failed",
  "protocol",
  "server_rejected",
  "read_only",
  "unsupported_server",
  "keychain",
  "account_exists",
  "missing_mailbox",
  "confirmation_required",
  "not_undoable",
  "invalid_keyword",
  "send_rejected",
  "send_unclear",
  "invalid_address",
  "attachment_too_large",
  "send_in_progress",
  "sync_wrong_passphrase",
  "sync_not_set_up",
  "sync_already_set_up",
  "sync_key_changed",
  "sync_key_invalid",
  "sync_off",
  "sync_key_missing",
  "send_missed",
  "message_too_large",
  "ai_off",
  "ai_consent_required",
  "ai_not_permitted",
  "ai_cancelled",
  "ai_unreachable",
  "ai_unauthorized",
  "ai_busy",
  "ai_refused",
  "ai_failed",
] as const satisfies readonly ErrorCode[];

// Fails to compile when the core adds an error code that is missing above.
type MissingErrorCode = Exclude<ErrorCode, (typeof ERROR_CODES)[number]>;
const errorCodesAreComplete: [MissingErrorCode] extends [never] ? true : never = true;
void errorCodesAreComplete;

/**
 * A failed core call. It carries the typed `IpcError` fields, so it can be used wherever an
 * `IpcError` is expected, and it is a real `Error` for stack traces and TanStack Query.
 */
export class CoreError extends Error implements IpcError {
  readonly code: ErrorCode;
  readonly detail: string | null;

  constructor(error: IpcError) {
    super(error.message);
    this.name = "CoreError";
    this.code = error.code;
    this.detail = error.detail;
  }
}

export function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    typeof v["code"] === "string" &&
    (ERROR_CODES as readonly string[]).includes(v["code"]) &&
    typeof v["message"] === "string" &&
    (v["detail"] === null || typeof v["detail"] === "string")
  );
}

const FRAMEWORK_ERROR_MESSAGE = "The app core rejected the request.";

/**
 * Framework failures (unknown command, bad argument, ACL denial) reject with a plain string,
 * not with an `IpcError`, even though the generated type claims otherwise. They become `internal`.
 */
export function toCoreError(value: unknown): CoreError {
  if (value instanceof CoreError) return value;
  if (isIpcError(value)) return new CoreError(value);
  const detail = value instanceof Error ? value.message : String(value);
  return new CoreError({ code: "internal", message: FRAMEWORK_ERROR_MESSAGE, detail });
}

type SpectaResult<T> = { status: "ok"; data: T } | { status: "error"; error: IpcError };

async function call<T>(start: () => Promise<SpectaResult<T>>): Promise<T> {
  let result: SpectaResult<T>;
  try {
    result = await start();
  } catch (e) {
    // The generated runtime rethrows `Error` instances (e.g. no Tauri runtime present).
    throw toCoreError(e);
  }
  if (result.status === "error") throw toCoreError(result.error);
  return result.data;
}

/** Commands without a result answer `null`; callers only need to know that they finished. */
async function done(result: Promise<null>): Promise<void> {
  await result;
}

export type Unsubscribe = () => void;

export const ipc = {
  getAppInfo: (): Promise<AppInfo> => call(() => commands.getAppInfo()),
  getPreferences: (): Promise<Preferences> => call(() => commands.getPreferences()),
  setPreferences: (preferences: Preferences): Promise<Preferences> =>
    call(() => commands.setPreferences(preferences)),

  listAccounts: (): Promise<Account[]> => call(() => commands.listAccounts()),
  /** Built-in provider table only; never touches the network. */
  discoverKnownProvider: (email: string): Promise<DiscoveryResult> =>
    call(() => commands.discoverKnownProvider(email)),
  /** Contacts DNS and `autoconfig.<domain>`: only call this after the user clicked for it. */
  discoverOnline: (email: string): Promise<DiscoveryResult> => call(() => commands.discoverOnline(email)),
  testConnection: (settings: AccountSettings, password: string): Promise<ConnectionTestResult> =>
    call(() => commands.testConnection(settings, password)),
  addAccount: (settings: AccountSettings, password: string): Promise<Account> =>
    call(() => commands.addAccount(settings, password)),
  removeAccount: (accountId: AccountId): Promise<void> => done(call(() => commands.removeAccount(accountId))),
  retryAccount: (accountId: AccountId): Promise<void> => done(call(() => commands.retryAccount(accountId))),
  updatePassword: (accountId: AccountId, password: string): Promise<void> =>
    done(call(() => commands.updatePassword(accountId, password))),
  listMailboxes: (accountId: AccountId): Promise<Mailbox[]> => call(() => commands.listMailboxes(accountId)),
  setMailboxRole: (mailboxId: MailboxId, role: MailboxRole): Promise<Mailbox[]> =>
    call(() => commands.setMailboxRole(mailboxId, role)),
  listMessages: (mailboxId: MailboxId, cursor: ListCursor | null, limit: number): Promise<MessagePage> =>
    call(() => commands.listMessages(mailboxId, cursor, limit)),

  /** Answers at once: a rendered body, `loading` (wait for `messageBodyAvailable`) or `unavailable`. */
  getMessage: (messageId: MessageId, format: BodyFormat): Promise<MessageView> =>
    call(() => commands.getMessage(messageId, format)),
  /** The core fetches the external images itself and renders again. `sender` also remembers the sender. */
  loadRemoteContent: (messageId: MessageId, scope: RemoteContentScope): Promise<MessageView> =>
    call(() => commands.loadRemoteContent(messageId, scope)),
  listRemoteContentSenders: (accountId: AccountId): Promise<string[]> =>
    call(() => commands.listRemoteContentSenders(accountId)),
  revokeRemoteContent: (accountId: AccountId, sender: string): Promise<void> =>
    done(call(() => commands.revokeRemoteContent(accountId, sender))),
  /** The real target of a clicked link. The UI never learns it any other way. */
  describeLink: (renderToken: RenderToken, index: number): Promise<LinkInfo> =>
    call(() => commands.describeLink(renderToken, index)),
  /**
   * Opens a link after the user confirmed the target: http(s) in the system browser
   * (`{ kind: "opened" }`); a `mailto:` link comes back as a compose request for the composer.
   */
  openLink: (renderToken: RenderToken, index: number): Promise<LinkOpened> =>
    call(() => commands.openLink(renderToken, index)),
  /** The core shows the system's save dialog. */
  saveAttachment: (messageId: MessageId, index: number): Promise<SaveAttachmentResult> =>
    call(() => commands.saveAttachment(messageId, index)),
  /** `confirmedExecutable` only after the user confirmed the warning for this attachment. */
  openAttachment: (messageId: MessageId, index: number, confirmedExecutable: boolean): Promise<OpenAttachmentResult> =>
    call(() => commands.openAttachment(messageId, index, confirmedExecutable)),
  getPrefetchSettings: (): Promise<PrefetchSettings> => call(() => commands.getPrefetchSettings()),
  setPrefetchSettings: (settings: PrefetchSettings): Promise<PrefetchSettings> =>
    call(() => commands.setPrefetchSettings(settings)),
  getPrefetchStatus: (accountId: AccountId): Promise<PrefetchStatus> => call(() => commands.getPrefetchStatus(accountId)),

  getReadingPreferences: (): Promise<ReadingPreferences> => call(() => commands.getReadingPreferences()),
  setReadingPreferences: (preferences: ReadingPreferences): Promise<ReadingPreferences> =>
    call(() => commands.setReadingPreferences(preferences)),

  /**
   * One user action on messages of one account (at most `MAX_ACTION_MESSAGES`). Local at once,
   * then sent to the server. `requestId` is a fresh UUID per user action: the same id twice
   * records nothing new.
   */
  applyAction: (action: MailAction, messages: MessageRef[], requestId: string): Promise<ActionResult> =>
    call(() => commands.applyAction(action, messages, requestId)),
  /** The "Undo" of the notice: cancels what is still queued, sends counter-operations for the rest. */
  undoAction: (batch: BatchId): Promise<ActionResult> => call(() => commands.undoAction(batch)),
  /** Queues the creation of a folder; it appears with `mailboxesChanged` once the server made it. */
  createMailbox: (accountId: AccountId, path: string, role: MailboxRole | null): Promise<ActionResult> =>
    call(() => commands.createMailbox(accountId, path, role)),
  /** Activity view, newest first, keyset-paged by the last `id`. `limit` 1..500. */
  listOperations: (accountId: AccountId, before: OperationId | null, limit: number): Promise<OperationInfo[]> =>
    call(() => commands.listOperations(accountId, before, limit)),
  retryOperation: (operationId: OperationId): Promise<ActionResult> => call(() => commands.retryOperation(operationId)),
  discardOperation: (operationId: OperationId): Promise<void> => done(call(() => commands.discardOperation(operationId))),
  getOutboxCounts: (accountId: AccountId): Promise<OutboxCounts> => call(() => commands.getOutboxCounts(accountId)),
  /** The user's per-account switch. Off: queued operations fail with `read_only` and are reverted. */
  setWritesEnabled: (accountId: AccountId, enabled: boolean): Promise<Account> =>
    call(() => commands.setWritesEnabled(accountId, enabled)),

  /** Subscribes to the typed event stream of the core. Resolves once the listener is registered. */
  onCoreEvent: async (handler: (event: CoreEvent) => void): Promise<Unsubscribe> => {
    try {
      return await events.coreEvent.listen((e) => {
        handler(e.payload);
      });
    } catch (e) {
      throw toCoreError(e);
    }
  },
};

// ---- Stage 12 (AI over MCP) and stage 11 (settings sync): commands, types, constants ----
import {
  commands as mcpSyncCommands,
  MAX_MCP_ACCESS_PAGE,
  MAX_MCP_CLIENT_NAME_CHARS,
  MAX_MCP_CLIENTS,
  MCP_ACCESS_MAX_ENTRIES,
  MCP_ACCESS_RETENTION_DAYS,
  MIN_SYNC_PASSPHRASE_CHARS,
  SETTINGS_SYNC_FOLDER,
  type McpAccessEntry,
  type McpAccessId,
  type McpAccessOutcome,
  type McpAccessPage,
  type McpAccountAccess,
  type McpAccountShare,
  type McpClientConfig,
  type McpClientCreated,
  type McpClientInfo,
  type McpClientKind,
  type McpConnectionTest,
  type McpHelperInfo,
  type McpHostState,
  type McpRefusal,
  type McpSettingsInfo,
  type McpSetup,
  type McpStatus,
  type McpTestProblem,
  type McpTool,
  type SettingsSyncPreview,
  type SettingsSyncState,
  type SettingsSyncStatus,
  type SyncDevice,
  type SyncKindCount,
  type SyncQuestion,
} from "../generated/bindings";

export {
  MAX_MCP_ACCESS_PAGE,
  MAX_MCP_CLIENT_NAME_CHARS,
  MAX_MCP_CLIENTS,
  MCP_ACCESS_MAX_ENTRIES,
  MCP_ACCESS_RETENTION_DAYS,
  MIN_SYNC_PASSPHRASE_CHARS,
  SETTINGS_SYNC_FOLDER,
};

export type {
  McpAccessEntry,
  McpAccessId,
  McpAccessOutcome,
  McpAccessPage,
  McpAccountAccess,
  McpAccountShare,
  McpClientConfig,
  McpClientCreated,
  McpClientInfo,
  McpClientKind,
  McpConnectionTest,
  McpHelperInfo,
  McpHostState,
  McpRefusal,
  McpSettingsInfo,
  McpSetup,
  McpStatus,
  McpTestProblem,
  McpTool,
  SettingsSyncPreview,
  SettingsSyncState,
  SettingsSyncStatus,
  SyncDevice,
  SyncKindCount,
  SyncQuestion,
};

/** Commands that cannot fail on the Rust side still reject on framework errors (no runtime, ACL). */
async function infallible<T>(start: () => Promise<T>): Promise<T> {
  try {
    return await start();
  } catch (e) {
    throw toCoreError(e);
  }
}

/** Stage 12: the local MCP server for AI clients. Off until the user turns it on. */
export const mcpIpc = {
  getSettings: (): Promise<McpSettingsInfo> => call(() => mcpSyncCommands.getMcpSettings()),
  /** `false` is the emergency stop: the endpoint goes, every open connection closes at once. */
  setEnabled: (enabled: boolean): Promise<McpSettingsInfo> => call(() => mcpSyncCommands.setMcpEnabled(enabled)),
  setAccountAccess: (accountId: AccountId, access: McpAccountAccess): Promise<McpSettingsInfo> =>
    call(() => mcpSyncCommands.setMcpAccountAccess(accountId, access)),
  /** The token and the filled-in configurations come only here, once. Never log or keep them. */
  createClient: (name: string): Promise<McpClientCreated> => call(() => mcpSyncCommands.createMcpClient(name)),
  revokeClient: (clientId: string): Promise<McpSettingsInfo> => call(() => mcpSyncCommands.revokeMcpClient(clientId)),
  listClients: (): Promise<McpClientInfo[]> => call(() => mcpSyncCommands.listMcpClients()),
  /** The configurations with the placeholder `<token>`. */
  getSetup: (): Promise<McpSetup> => infallible(() => mcpSyncCommands.getMcpSetup()),
  /** Access log, newest first, keyset-paged by `nextCursor`. `limit` 1..`MAX_MCP_ACCESS_PAGE`. */
  listAccess: (before: McpAccessId | null, limit: number): Promise<McpAccessPage> =>
    call(() => mcpSyncCommands.listMcpAccess(before, limit)),
  getStatus: (): Promise<McpStatus> => infallible(() => mcpSyncCommands.getMcpStatus()),
  /** Starts the helper with a temporary client; takes about a second, at most 20 s. */
  testConnection: (): Promise<McpConnectionTest> => call(() => mcpSyncCommands.testMcpConnection()),
};

/** Stage 11: settings sync through an encrypted folder in one of the user's mailboxes. */
export const settingsSyncIpc = {
  getStatus: (): Promise<SettingsSyncStatus> => call(() => mcpSyncCommands.getSettingsSyncStatus()),
  /** First device. `sync_already_set_up` means: join instead. */
  enable: (accountId: AccountId, passphrase: string): Promise<SettingsSyncStatus> =>
    call(() => mcpSyncCommands.enableSettingsSync(accountId, passphrase)),
  /** Checks the passphrase and plans the merge; writes nothing. The key waits for `join`. */
  previewJoin: (accountId: AccountId, passphrase: string): Promise<SettingsSyncPreview> =>
    call(() => mcpSyncCommands.previewSettingsSyncJoin(accountId, passphrase)),
  join: (): Promise<SettingsSyncStatus> => call(() => mcpSyncCommands.joinSettingsSync()),
  syncNow: (): Promise<SettingsSyncStatus> => call(() => mcpSyncCommands.syncSettingsNow()),
  resolveQuestion: (accountId: AccountId, remove: boolean): Promise<SettingsSyncStatus> =>
    call(() => mcpSyncCommands.resolveSyncQuestion(accountId, remove)),
  changePassphrase: (passphrase: string): Promise<void> =>
    done(call(() => mcpSyncCommands.changeSettingsSyncPassphrase(passphrase))),
  /** New key from this device; the other devices need the new passphrase. */
  reset: (passphrase: string): Promise<SettingsSyncStatus> => call(() => mcpSyncCommands.resetSettingsSync(passphrase)),
  disable: (): Promise<SettingsSyncStatus> => call(() => mcpSyncCommands.disableSettingsSync()),
  setDeviceName: (name: string): Promise<SettingsSyncStatus> => call(() => mcpSyncCommands.setSyncDeviceName(name)),
};

// ---- Stage 7 (search, threads, unified inbox, account groups, identities) ----
import {
  commands as searchCommands,
  MAX_SEARCH_PAGE,
  MAX_THREAD_MESSAGES,
  type AccountGroup,
  type AccountGroupId,
  type IdentityId,
  type IdentityInfo,
  type IdentityInput,
  type SearchHighlights,
  type SearchHit,
  type SearchIndexState,
  type SearchIndexStatus,
  type SearchPage,
  type SearchProblem,
  type SearchProblemCode,
  type SearchRequest,
  type SearchScope,
  type SearchTermInfo,
  type SearchTermKind,
  type ServerSearchResult,
  type TextRange,
  type ThreadEntry,
  type ThreadListItem,
  type ThreadPage,
  type ThreadView,
  type UnifiedView,
} from "../generated/bindings";

export { MAX_SEARCH_PAGE, MAX_THREAD_MESSAGES };

export type {
  AccountGroup,
  AccountGroupId,
  IdentityId,
  IdentityInfo,
  IdentityInput,
  SearchHighlights,
  SearchHit,
  SearchIndexState,
  SearchIndexStatus,
  SearchPage,
  SearchProblem,
  SearchProblemCode,
  SearchRequest,
  SearchScope,
  SearchTermInfo,
  SearchTermKind,
  ServerSearchResult,
  TextRange,
  ThreadEntry,
  ThreadListItem,
  ThreadPage,
  ThreadView,
  UnifiedView,
};

/** Stage 7: full-text search, conversations, the unified inbox, account groups and identities. */
export const searchIpc = {
  /** Local full-text search. Problems of the query come back in the page, not as an error. */
  search: (request: SearchRequest): Promise<SearchPage> => call(() => searchCommands.search(request)),
  /** IMAP SEARCH over the read-only connection, for messages whose body is not stored locally. */
  searchOnServer: (request: SearchRequest): Promise<ServerSearchResult> =>
    call(() => searchCommands.searchOnServer(request)),
  getSearchIndexStatus: (): Promise<SearchIndexStatus> => call(() => searchCommands.getSearchIndexStatus()),
  rebuildSearchIndex: (): Promise<SearchIndexStatus> => call(() => searchCommands.rebuildSearchIndex()),

  /** Conversations of one folder, newest first. `limit` 1..500. */
  listThreads: (mailboxId: MailboxId, cursor: ListCursor | null, limit: number): Promise<ThreadPage> =>
    call(() => searchCommands.listThreads(mailboxId, cursor, limit)),
  listUnifiedThreads: (view: UnifiedView, cursor: ListCursor | null, limit: number): Promise<ThreadPage> =>
    call(() => searchCommands.listUnifiedThreads(view, cursor, limit)),
  /** Open a thread by one of its messages, never by a stored thread id (ids change on merges). */
  getThread: (messageId: MessageId, mailboxId: MailboxId | null): Promise<ThreadView> =>
    call(() => searchCommands.getThread(messageId, mailboxId)),
  /** The inboxes of every visible account (or of a group), mixed by date. */
  listUnifiedInbox: (view: UnifiedView, cursor: ListCursor | null, limit: number): Promise<MessagePage> =>
    call(() => searchCommands.listUnifiedInbox(view, cursor, limit)),

  listAccountGroups: (): Promise<AccountGroup[]> => call(() => searchCommands.listAccountGroups()),
  createAccountGroup: (name: string, accountIds: AccountId[]): Promise<AccountGroup> =>
    call(() => searchCommands.createAccountGroup(name, accountIds)),
  /** Replaces name and members. */
  updateAccountGroup: (groupId: AccountGroupId, name: string, accountIds: AccountId[]): Promise<AccountGroup> =>
    call(() => searchCommands.updateAccountGroup(groupId, name, accountIds)),
  deleteAccountGroup: (groupId: AccountGroupId): Promise<void> =>
    done(call(() => searchCommands.deleteAccountGroup(groupId))),
  /** On this device only; the account keeps syncing. */
  listHiddenAccounts: (): Promise<AccountId[]> => call(() => searchCommands.listHiddenAccounts()),
  setAccountHidden: (accountId: AccountId, hidden: boolean): Promise<void> =>
    done(call(() => searchCommands.setAccountHidden(accountId, hidden))),

  /** The default first. */
  listIdentities: (accountId: AccountId): Promise<IdentityInfo[]> => call(() => searchCommands.listIdentities(accountId)),
  addIdentity: (accountId: AccountId, identity: IdentityInput): Promise<IdentityInfo> =>
    call(() => searchCommands.addIdentity(accountId, identity)),
  updateIdentity: (identityId: IdentityId, identity: IdentityInput): Promise<IdentityInfo> =>
    call(() => searchCommands.updateIdentity(identityId, identity)),
  /** Answers with all identities of the account, the new default first. */
  setDefaultIdentity: (identityId: IdentityId): Promise<IdentityInfo[]> =>
    call(() => searchCommands.setDefaultIdentity(identityId)),
  /** The default identity cannot be deleted (`invalid_argument`). */
  deleteIdentity: (identityId: IdentityId): Promise<void> => done(call(() => searchCommands.deleteIdentity(identityId))),
  getReplyIdentity: (messageId: MessageId): Promise<IdentityInfo> => call(() => searchCommands.getReplyIdentity(messageId)),
};

// ---- Stage 5 (composer, drafts, sending, outbox view, compose settings) ----
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  commands as composeCommands,
  DEFAULT_SEND_DELAY_SECS,
  LARGE_ATTACHMENT_WARNING_BYTES,
  MAX_ATTACHMENT_BYTES,
  MAX_ATTACHMENTS,
  MAX_CONTACT_SUGGESTIONS,
  MAX_SEND_DELAY_SECS,
  type AccountSendSettings,
  type ComposeFormat,
  type ComposeIdentity,
  type ComposeSettings,
  type ContactSuggestion,
  type Draft,
  type DraftAttachment,
  type DraftId,
  type DraftKind,
  type DraftServerState,
  type DraftState,
  type DraftSummary,
  type DraftUpdate,
  type NewDraft,
  type OutgoingMail,
  type QuoteLanguage,
  type QuotePosition,
  type SendState,
  type SentCopyMode,
  type UnclearDecision,
} from "../generated/bindings";

export {
  DEFAULT_SEND_DELAY_SECS,
  LARGE_ATTACHMENT_WARNING_BYTES,
  MAX_ATTACHMENT_BYTES,
  MAX_ATTACHMENTS,
  MAX_CONTACT_SUGGESTIONS,
  MAX_SEND_DELAY_SECS,
};

export type {
  AccountSendSettings,
  ComposeFormat,
  ComposeIdentity,
  ComposeSettings,
  ContactSuggestion,
  Draft,
  DraftAttachment,
  DraftId,
  DraftKind,
  DraftServerState,
  DraftState,
  DraftSummary,
  DraftUpdate,
  NewDraft,
  OutgoingMail,
  QuoteLanguage,
  QuotePosition,
  SendState,
  SentCopyMode,
  UnclearDecision,
};

/** Files dragged over the window, as the shell reports them (paths only on `enter` and `drop`). */
export type FileDragEvent = { type: "enter"; paths: string[] } | { type: "over" } | { type: "drop"; paths: string[] } | { type: "leave" };

/**
 * Drafts and sending (docs/decisions/0018). Drafts are saved by the composer (`saveDraft`
 * debounced); the core sends the server copy itself. Sending goes through the outbox and waits
 * for the undo delay first.
 */
export const composeIpc = {
  /** A new, reply, reply-all or forward draft (recipients, identity, quote prepared by the core). */
  createDraft: (request: NewDraft): Promise<Draft> => call(() => composeCommands.createDraft(request)),
  getDraft: (draftId: DraftId): Promise<Draft> => call(() => composeCommands.getDraft(draftId)),
  /** Local save; call it debounced while typing and on blur. `send_in_progress` while frozen. */
  saveDraft: (draftId: DraftId, update: DraftUpdate): Promise<Draft> =>
    call(() => composeCommands.saveDraft(draftId, update)),
  /** The composer closed: the server copy follows now. Call after a final `saveDraft`. */
  closeDraft: (draftId: DraftId): Promise<Draft> => call(() => composeCommands.closeDraft(draftId)),
  /** Discards the draft, its files and its server copy. */
  deleteDraft: (draftId: DraftId): Promise<void> => done(call(() => composeCommands.deleteDraft(draftId))),
  listDrafts: (accountId: AccountId | null): Promise<DraftSummary[]> => call(() => composeCommands.listDrafts(accountId)),
  /** A message of the Drafts folder as a local draft (also one of another program). */
  openDraftFromMessage: (messageId: MessageId): Promise<Draft> =>
    call(() => composeCommands.openDraftFromMessage(messageId)),
  /** The system file dialog (in Rust); cancelling answers the unchanged draft. */
  pickAttachments: (draftId: DraftId): Promise<Draft> => call(() => composeCommands.pickAttachments(draftId)),
  /** Only paths of a drop the shell saw itself (`onFileDrop`), shortly before. */
  addDroppedAttachments: (draftId: DraftId, paths: string[]): Promise<Draft> =>
    call(() => composeCommands.addDroppedAttachments(draftId, paths)),
  removeDraftAttachment: (draftId: DraftId, attachmentId: string): Promise<Draft> =>
    call(() => composeCommands.removeDraftAttachment(draftId, attachmentId)),
  /** `requestId`: a fresh UUID per press of "Send" (the same id twice sends once). */
  sendDraft: (draftId: DraftId, requestId: string): Promise<OutgoingMail> =>
    call(() => composeCommands.sendDraft(draftId, requestId)),
  /** Undo within the delay, or "Edit" of a failed or unclear send: the draft is editable again. */
  cancelSend: (draftId: DraftId): Promise<Draft> => call(() => composeCommands.cancelSend(draftId)),
  /** The outbox view of an account, newest first, keyset-paged by `operationId`. `limit` 1..500. */
  listOutgoing: (accountId: AccountId, before: OperationId | null, limit: number): Promise<OutgoingMail[]> =>
    call(() => composeCommands.listOutgoing(accountId, before, limit)),
  /** The user's answer to `sendNeedsDecision`; `send_again` only after an explicit confirmation. */
  resolveUnclearSend: (operationId: OperationId, decision: UnclearDecision): Promise<OutgoingMail> =>
    call(() => composeCommands.resolveUnclearSend(operationId, decision)),
  getComposeSettings: (): Promise<ComposeSettings> => call(() => composeCommands.getComposeSettings()),
  setComposeSettings: (settings: ComposeSettings): Promise<ComposeSettings> =>
    call(() => composeCommands.setComposeSettings(settings)),
  getSendSettings: (accountId: AccountId): Promise<AccountSendSettings> => call(() => composeCommands.getSendSettings(accountId)),
  setSentCopyMode: (accountId: AccountId, mode: SentCopyMode): Promise<AccountSendSettings> =>
    call(() => composeCommands.setSentCopyMode(accountId, mode)),
  listComposeIdentities: (accountId: AccountId): Promise<ComposeIdentity[]> =>
    call(() => composeCommands.listComposeIdentities(accountId)),
  /** Plain text without "-- "; null or blank removes it. */
  setIdentitySignature: (identityId: IdentityId, signature: string | null): Promise<ComposeIdentity> =>
    call(() => composeCommands.setIdentitySignature(identityId, signature)),
  /** Autocomplete from addresses of earlier mail. `limit` 1..`MAX_CONTACT_SUGGESTIONS`. */
  searchContacts: (query: string, limit: number): Promise<ContactSuggestion[]> =>
    call(() => composeCommands.searchContacts(query, limit)),
  /**
   * Files dragged onto the window, from the shell's own drag-and-drop event (the page cannot fake
   * the paths the core accepts). Without a Tauri window (tests) nothing is reported.
   */
  onFileDrop: async (handler: (event: FileDragEvent) => void): Promise<Unsubscribe> => {
    try {
      return await getCurrentWebview().onDragDropEvent((e) => {
        const payload = e.payload;
        if (payload.type === "enter" || payload.type === "drop") handler({ type: payload.type, paths: payload.paths });
        else handler({ type: payload.type });
      });
    } catch {
      return () => undefined;
    }
  },
};

// ---- Stage 9 (inbox comfort: categories, bundles, snooze, pins, tasks, rules, notifications) ----
import {
  commands as comfortCommands,
  type BadgeMode,
  type BundleKey,
  type BundleSummary,
  type CategoryDecision,
  type CategoryReason,
  type DateSection,
  type DateSectionKind,
  type InboxCursor,
  type InboxFilter,
  type InboxPage,
  type InboxRow,
  type InboxScope,
  type InboxSettings,
  type InboxTop,
  type MailRule,
  type MarkScope,
  type MessageCategory,
  type MessageCategoryInfo,
  type NotificationSettings,
  type NotificationTexts,
  type NotifyCategories,
  type QuietHours,
  type RuleAction,
  type RuleCondition,
  type RuleId,
  type RuleInput,
  type RuleSkipReason,
  type SenderChip,
  type SenderKey,
  type SenderKeyKind,
  type SenderRule,
  type SnoozeChoice,
  type SnoozeMark,
  type SnoozeResult,
  type SnoozeTimes,
  type TaskFilter,
  type TaskMark,
  type TextMatchOp,
} from "../generated/bindings";

export type {
  BadgeMode,
  BundleKey,
  BundleSummary,
  CategoryDecision,
  CategoryReason,
  DateSection,
  DateSectionKind,
  InboxCursor,
  InboxFilter,
  InboxPage,
  InboxRow,
  InboxScope,
  InboxSettings,
  InboxTop,
  MailRule,
  MarkScope,
  MessageCategory,
  MessageCategoryInfo,
  NotificationSettings,
  NotificationTexts,
  NotifyCategories,
  QuietHours,
  RuleAction,
  RuleCondition,
  RuleId,
  RuleInput,
  RuleSkipReason,
  SenderChip,
  SenderKey,
  SenderKeyKind,
  SenderRule,
  SnoozeChoice,
  SnoozeMark,
  SnoozeResult,
  SnoozeTimes,
  TaskFilter,
  TaskMark,
  TextMatchOp,
};

/** Stage 9 commands without a typed error on the Rust side still reject on framework errors. */
async function comfortPlain<T>(start: () => Promise<T>): Promise<T> {
  try {
    return await start();
  } catch (e) {
    throw toCoreError(e);
  }
}

/** Stage 9: the sorted inbox, snooze, pins, tasks, sender and local rules, notifications. */
export const comfortIpc = {
  /** One page of rows (messages, bundles, continued bundles); `top` only without a cursor. `limit` counts messages. */
  listInbox: (scope: InboxScope, cursor: InboxCursor | null, limit: number): Promise<InboxPage> =>
    call(() => comfortCommands.listInbox(scope, cursor, limit)),
  /** One category of the inbox, or the important senders ("Show all"). */
  listInboxFiltered: (scope: InboxScope, filter: InboxFilter, cursor: ListCursor | null, limit: number): Promise<MessagePage> =>
    call(() => comfortCommands.listInboxFiltered(scope, filter, cursor, limit)),
  getInboxSettings: (): Promise<InboxSettings> => call(() => comfortCommands.getInboxSettings()),
  /** `inboxChanged` follows. */
  setInboxSettings: (settings: InboxSettings): Promise<InboxSettings> =>
    call(() => comfortCommands.setInboxSettings(settings)),

  /** "Why here?" and the keys "always as …" can use. */
  getMessageCategory: (messageId: MessageId): Promise<MessageCategoryInfo> =>
    call(() => comfortCommands.getMessageCategory(messageId)),
  /** `null` removes the correction. Applied to every message in the background, then `inboxChanged`. */
  setSenderCategory: (key: SenderKey, category: MessageCategory | null): Promise<SenderRule | null> =>
    call(() => comfortCommands.setSenderCategory(key, category)),
  setSenderImportant: (key: SenderKey, important: boolean): Promise<SenderRule | null> =>
    call(() => comfortCommands.setSenderImportant(key, important)),
  listSenderRules: (): Promise<SenderRule[]> => call(() => comfortCommands.listSenderRules()),
  deleteSenderRule: (id: string): Promise<void> => done(call(() => comfortCommands.deleteSenderRule(id))),

  /** The times the snooze menu offers now (system time zone). */
  getSnoozeTimes: (): Promise<SnoozeTimes> => comfortPlain(() => comfortCommands.getSnoozeTimes()),
  /** Local, plus a `$Snoozed` keyword through the outbox where writes are on. Never moves mail. */
  snoozeMessages: (messageIds: MessageId[], choice: SnoozeChoice): Promise<SnoozeResult> =>
    call(() => comfortCommands.snoozeMessages(messageIds, choice)),
  /** Waiting ones come back now; returned ones lose their mark. */
  unsnoozeMessages: (messageIds: MessageId[]): Promise<void> =>
    done(call(() => comfortCommands.unsnoozeMessages(messageIds))),
  /** Local only, no keyword on the server. */
  setPinned: (messageIds: MessageId[], pinned: boolean): Promise<void> =>
    done(call(() => comfortCommands.setPinned(messageIds, pinned))),
  /** A task, done, or no task (`null`). */
  setTask: (messageIds: MessageId[], mark: TaskMark | null): Promise<void> =>
    done(call(() => comfortCommands.setTask(messageIds, mark))),
  listSnoozed: (scope: MarkScope): Promise<MessageListItem[]> => call(() => comfortCommands.listSnoozed(scope)),
  listPinned: (scope: MarkScope): Promise<MessageListItem[]> => call(() => comfortCommands.listPinned(scope)),
  listTasks: (scope: MarkScope, filter: TaskFilter): Promise<MessageListItem[]> =>
    call(() => comfortCommands.listTasks(scope, filter)),

  /** Local rules, by position. They run only while the app runs. */
  listRules: (): Promise<MailRule[]> => call(() => comfortCommands.listRules()),
  createRule: (input: RuleInput): Promise<MailRule> => call(() => comfortCommands.createRule(input)),
  updateRule: (id: RuleId, input: RuleInput): Promise<MailRule> => call(() => comfortCommands.updateRule(id, input)),
  deleteRule: (id: RuleId): Promise<void> => done(call(() => comfortCommands.deleteRule(id))),
  reorderRules: (ids: RuleId[]): Promise<MailRule[]> => call(() => comfortCommands.reorderRules(ids)),

  getNotificationSettings: (): Promise<NotificationSettings> => call(() => comfortCommands.getNotificationSettings()),
  setNotificationSettings: (settings: NotificationSettings): Promise<NotificationSettings> =>
    call(() => comfortCommands.setNotificationSettings(settings)),
  /** The texts of system notifications in the UI's language: at start and on every language change. */
  setNotificationTexts: (texts: NotificationTexts): Promise<void> =>
    comfortPlain(() => comfortCommands.setNotificationTexts(texts)),
};

// ---- Stage 9 part B (extras: unsubscribe, invitations, export, print, default mail app, mailto) ----
import {
  commands as extrasCommands,
  type AttendeeRole,
  type CalendarEvent,
  type CalendarInvite,
  type CalendarInviteInfo,
  type CalendarPerson,
  type CalendarProblem,
  type CalendarProblemCode,
  type CalendarRecurrence,
  type CalendarTime,
  type ComposeRequest,
  type DefaultMailApp,
  type DefaultMailAppOutcome,
  type EventStatus,
  type MailtoField,
  type MailtoNotice,
  type MessageExtras,
  type ParticipationStatus,
  type RecurrenceFrequency,
  type UnsubscribeMethod,
  type UnsubscribeOptions,
  type UnsubscribeOutcome,
} from "../generated/bindings";

export type {
  AttendeeRole,
  CalendarEvent,
  CalendarInvite,
  CalendarInviteInfo,
  CalendarPerson,
  CalendarProblem,
  CalendarProblemCode,
  CalendarRecurrence,
  CalendarTime,
  ComposeRequest,
  DefaultMailApp,
  DefaultMailAppOutcome,
  EventStatus,
  MailtoField,
  MailtoNotice,
  MessageExtras,
  ParticipationStatus,
  RecurrenceFrequency,
  UnsubscribeMethod,
  UnsubscribeOptions,
  UnsubscribeOutcome,
};

/** Stage 9 part B commands without a typed error on the Rust side still reject on framework errors. */
async function extrasPlain<T>(start: () => Promise<T>): Promise<T> {
  try {
    return await start();
  } catch (e) {
    throw toCoreError(e);
  }
}

/**
 * Stage 9 part B: what the reading pane offers besides the body (unsubscribing, calendar
 * invitations, `.eml` export, printing), the default mail app and `mailto:` links.
 */
export const extrasIpc = {
  /** Unsubscribe options and invitations (these only once `bodyComplete`), plus the category. */
  getMessageExtras: (messageId: MessageId): Promise<MessageExtras> =>
    call(() => extrasCommands.getMessageExtras(messageId)),
  /**
   * Only after the user's click: `oneClick` POSTs (RFC 8058), `mailto` answers the composer's
   * fields (nothing is sent), `web` opens the page in the system browser after the UI asked.
   */
  unsubscribe: (messageId: MessageId, method: UnsubscribeMethod): Promise<UnsubscribeOutcome> =>
    call(() => extrasCommands.unsubscribe(messageId, method)),
  /** Hands the calendar part to the system's calendar app (a read-only `.ics`). */
  openInvite: (messageId: MessageId, partId: number): Promise<void> =>
    done(call(() => extrasCommands.openInvite(messageId, partId))),
  /** The system's save dialog, then the raw message as `.eml`; null when the user cancelled. */
  exportMessage: (messageId: MessageId): Promise<string | null> => call(() => extrasCommands.exportMessage(messageId)),
  /** The system's print dialog for the window ("Save as PDF" is there); print styles pick the message. */
  printWindow: (): Promise<void> => done(call(() => extrasCommands.printWindow())),
  getDefaultMailApp: (): Promise<DefaultMailApp> => extrasPlain(() => extrasCommands.getDefaultMailApp()),
  setDefaultMailApp: (): Promise<DefaultMailAppOutcome> => extrasPlain(() => extrasCommands.setDefaultMailApp()),
  /** The `mailto:` links the app was started with; once at start, later ones come as `composeRequested`. */
  takeLaunchLinks: (): Promise<ComposeRequest[]> => extrasPlain(() => extrasCommands.takeLaunchLinks()),
};

// ---- Stage 10 (composer complete: send later, size, inline images, paste, signatures, templates, contacts) ----
import {
  commands as stage10Commands,
  type CleanedPaste,
  type ContactAddressInfo,
  type ContactDuplicate,
  type ContactEntry,
  type ContactMergeRequest,
  type ContactPage,
  type DraftSizeCheck,
  type DuplicateReason,
  type FilledTemplateInfo,
  type IdentitySignatureDefaults,
  type InlineImagesAdded,
  type MissedSendDecision,
  type SanitizeReport,
  type SavedSignature,
  type SavedTemplate,
  type SendLater,
  type SignatureId,
  type SignatureImageData,
  type SignatureImageInfo,
  type SignatureInfo,
  type SignatureInput,
  type TemplateFillProblem,
  type TemplateId,
  type TemplateInfo,
  type TemplateInput,
} from "../generated/bindings";

export type {
  CleanedPaste,
  ContactAddressInfo,
  ContactDuplicate,
  ContactEntry,
  ContactMergeRequest,
  ContactPage,
  DraftSizeCheck,
  DuplicateReason,
  FilledTemplateInfo,
  IdentitySignatureDefaults,
  InlineImagesAdded,
  MissedSendDecision,
  SanitizeReport,
  SavedSignature,
  SavedTemplate,
  SendLater,
  SignatureId,
  SignatureImageData,
  SignatureImageInfo,
  SignatureInfo,
  SignatureInput,
  TemplateFillProblem,
  TemplateId,
  TemplateInfo,
  TemplateInput,
};

/** Stage 10 limits of the core that the bindings do not export (decision 0023). */
export const MISSED_SEND_GRACE_MS = 3_600_000;
/** "Send later" at most this far ahead (366 days). */
export const MAX_SCHEDULE_AHEAD_MS = 366 * 24 * 3_600_000;
/** One inline image (8 MiB). */
export const MAX_INLINE_IMAGE_BYTES = 8 * 1024 * 1024;
/** Pasted HTML above this goes in as plain text (the core refuses it with `invalid_argument`). */
export const MAX_PASTE_BYTES = 2 * 1024 * 1024;
/** `listContacts` takes 1..200 per page. */
export const MAX_CONTACT_PAGE = 200;

/**
 * Stage 10, the composer's own commands: send later (local: only while the app runs and is
 * online), the SMTP size check, inline images, cleaning pasted HTML and forwarding as `.eml`
 * (`createDraft` with `forward_as_attachment`).
 */
export const composerIpc = {
  /** `requestId`: a fresh UUID per click. `sendAt` in the future, at most 366 days ahead. */
  scheduleSend: (draftId: DraftId, requestId: string, when: SendLater): Promise<OutgoingMail> =>
    call(() => stage10Commands.scheduleSend(draftId, requestId, when)),
  /** Waiting and missed scheduled sends of every account, earliest first. Ask once at start. */
  listScheduledSends: (): Promise<OutgoingMail[]> => call(() => stage10Commands.listScheduledSends()),
  /** A new time, or `null` = send now (after the usual undo delay). */
  rescheduleSend: (operationId: OperationId, when: SendLater | null): Promise<OutgoingMail> =>
    call(() => stage10Commands.rescheduleSend(operationId, when)),
  /** The answer to a missed send; `null` after `keep_as_draft`. */
  resolveMissedSend: (operationId: OperationId, decision: MissedSendDecision): Promise<OutgoingMail | null> =>
    call(() => stage10Commands.resolveMissedSend(operationId, decision)),
  /** The built message against the SMTP server's SIZE limit (the first call may take up to 15 s). */
  checkDraftSize: (draftId: DraftId): Promise<DraftSizeCheck> => call(() => stage10Commands.checkDraftSize(draftId)),
  /** The system dialog with an image filter (in Rust); cancelling answers `added: []`. */
  pickInlineImages: (draftId: DraftId): Promise<InlineImagesAdded> => call(() => stage10Commands.pickInlineImages(draftId)),
  /** Only paths of a drop the shell saw itself (`onFileDrop`). */
  addDroppedInlineImages: (draftId: DraftId, paths: string[]): Promise<InlineImagesAdded> =>
    call(() => stage10Commands.addDroppedInlineImages(draftId, paths)),
  /** A `data:` URL to show an inline image in the editor; never saved into the body. */
  inlineImageData: (draftId: DraftId, attachmentId: string): Promise<string> =>
    call(() => stage10Commands.inlineImageData(draftId, attachmentId)),
  /** Pasted HTML as editor HTML: no script, style, image or external reference. */
  cleanPastedHtml: (html: string): Promise<CleanedPaste> => call(() => stage10Commands.cleanPastedHtml(html)),
};

/** Stage 10: the signature pool, the defaults per identity, and templates with placeholders. */
export const snippetsIpc = {
  listSignatures: (): Promise<SignatureInfo[]> => call(() => stage10Commands.listSignatures()),
  saveSignature: (signature: SignatureInput): Promise<SavedSignature> => call(() => stage10Commands.saveSignature(signature)),
  deleteSignature: (signatureId: SignatureId): Promise<void> => done(call(() => stage10Commands.deleteSignature(signatureId))),
  getIdentitySignatureDefaults: (identityId: IdentityId): Promise<IdentitySignatureDefaults> =>
    call(() => stage10Commands.getIdentitySignatureDefaults(identityId)),
  setIdentitySignatureDefaults: (
    identityId: IdentityId,
    newSignature: SignatureId | null,
    replySignature: SignatureId | null,
  ): Promise<IdentitySignatureDefaults> =>
    call(() => stage10Commands.setIdentitySignatureDefaults(identityId, newSignature, replySignature)),
  /** The system dialog (in Rust); new images are the last entries. Save the signature first. */
  pickSignatureImages: (signatureId: SignatureId): Promise<SignatureInfo> =>
    call(() => stage10Commands.pickSignatureImages(signatureId)),
  addDroppedSignatureImages: (signatureId: SignatureId, paths: string[]): Promise<SignatureInfo> =>
    call(() => stage10Commands.addDroppedSignatureImages(signatureId, paths)),
  removeSignatureImage: (signatureId: SignatureId, contentId: string): Promise<SignatureInfo> =>
    call(() => stage10Commands.removeSignatureImage(signatureId, contentId)),
  signatureImageData: (signatureId: SignatureId): Promise<SignatureImageData[]> =>
    call(() => stage10Commands.signatureImageData(signatureId)),
  listTemplates: (): Promise<TemplateInfo[]> => call(() => stage10Commands.listTemplates()),
  saveTemplate: (template: TemplateInput): Promise<SavedTemplate> => call(() => stage10Commands.saveTemplate(template)),
  deleteTemplate: (templateId: TemplateId): Promise<void> => done(call(() => stage10Commands.deleteTemplate(templateId))),
  /** Fills the placeholders for this draft (first To recipient, the draft's identity); the draft is unchanged. */
  fillTemplate: (templateId: TemplateId, draftId: DraftId, language: QuoteLanguage): Promise<FilledTemplateInfo> =>
    call(() => stage10Commands.fillTemplate(templateId, draftId, language)),
};

/** Stage 10: the local contact directory (ranked by frequency and recency). */
export const contactsIpc = {
  /** `limit` 1..`MAX_CONTACT_PAGE`; `cursor` from the previous page. */
  listContacts: (query: string | null, cursor: string | null, limit: number): Promise<ContactPage> =>
    call(() => stage10Commands.listContacts(query, cursor, limit)),
  getContact: (email: string): Promise<ContactEntry> => call(() => stage10Commands.getContact(email)),
  /** `null` = back to the name seen most in mail. */
  setContactName: (email: string, name: string | null): Promise<ContactEntry> =>
    call(() => stage10Commands.setContactName(email, name)),
  listContactDuplicates: (): Promise<ContactDuplicate[]> => call(() => stage10Commands.listContactDuplicates()),
  /** Never suggested again on this device. */
  dismissContactDuplicate: (contacts: string[]): Promise<void> =>
    done(call(() => stage10Commands.dismissContactDuplicate(contacts))),
  /** Only after the user confirmed in a dialog (`confirmed: true`), else `confirmation_required`. */
  mergeContacts: (request: ContactMergeRequest): Promise<ContactEntry> => call(() => stage10Commands.mergeContacts(request)),
  separateContact: (email: string): Promise<ContactEntry> => call(() => stage10Commands.separateContact(email)),
};

// ---- Stage 13 (AI in the app: settings, consent, request log, panel, writing help, open replies) ----
import {
  commands as aiCommands,
  type AiAccountCloud,
  type AiAnswer,
  type AiAvailability,
  type AiClaim,
  type AiConnectionTest,
  type AiDeadline,
  type AiFunction,
  type AiFunctionChoice,
  type AiKeyChange,
  type AiLocality,
  type AiModel,
  type AiPreset,
  type AiProfileInfo,
  type AiProfileInput,
  type AiProviderKind,
  type AiReplySuggestion,
  type AiRequestEntry,
  type AiRequestId,
  type AiRequestOutcome,
  type AiRequestPage,
  type AiRequestPhase,
  type AiSettingsInfo,
  type AiSummary,
  type AiTargetLanguage,
  type AiTone,
  type AiTransfer,
  type AiTrimming,
  type AiWritingResult,
  type AiWritingTask,
  type OpenReplyHint,
  type OpenReplyKind,
  type OpenReplySettings,
} from "../generated/bindings";

export type {
  AiAccountCloud,
  AiAnswer,
  AiAvailability,
  AiClaim,
  AiConnectionTest,
  AiDeadline,
  AiFunction,
  AiFunctionChoice,
  AiKeyChange,
  AiLocality,
  AiModel,
  AiPreset,
  AiProfileInfo,
  AiProfileInput,
  AiProviderKind,
  AiReplySuggestion,
  AiRequestEntry,
  AiRequestId,
  AiRequestOutcome,
  AiRequestPage,
  AiRequestPhase,
  AiSettingsInfo,
  AiSummary,
  AiTargetLanguage,
  AiTone,
  AiTransfer,
  AiTrimming,
  AiWritingResult,
  AiWritingTask,
  OpenReplyHint,
  OpenReplyKind,
  OpenReplySettings,
};

/** Stage 13 limits of the core that the bindings do not export (IPC contract). */
export const AI_LIMITS = {
  /** `aiWritingHelp`: characters of the passage. */
  passageChars: 12_000,
  /** `aiAsk`: characters of the question. */
  questionChars: 1_000,
  /** `saveAiProfile`: characters of the name. */
  profileNameChars: 100,
  /** `setOpenReplySettings`: days. */
  openReplyDays: { min: 1, max: 30 },
  /** `listAiRequests`: entries per page. */
  requestPage: 500,
  /** The request log keeps this many days and entries. */
  logRetentionDays: 30,
  logMaxEntries: 10_000,
} as const;

/**
 * Stage 13: the optional built-in AI. Off until the user turns it on; while off every function
 * (also the model list and the connection test) fails with `ai_off` before anything leaves the
 * device. A key only ever goes UI → core (`AiKeyChange.set`); no answer contains one.
 */
export const aiIpc = {
  getSettings: (): Promise<AiSettingsInfo> => call(() => aiCommands.getAiSettings()),
  /** Which functions are usable now; the AI panel is hidden without any. */
  getAvailability: (): Promise<AiAvailability> => call(() => aiCommands.getAiAvailability()),
  /** Off also cancels running requests. */
  setEnabled: (enabled: boolean): Promise<AiSettingsInfo> => call(() => aiCommands.setAiEnabled(enabled)),
  saveProfile: (profile: AiProfileInput): Promise<AiSettingsInfo> => call(() => aiCommands.saveAiProfile(profile)),
  /** Functions using it become off; its key leaves the keychain. */
  deleteProfile: (profileId: string): Promise<AiSettingsInfo> => call(() => aiCommands.deleteAiProfile(profileId)),
  /** `null` turns the function off. */
  setFunction: (aiFunction: AiFunction, profileId: string | null): Promise<AiSettingsInfo> =>
    call(() => aiCommands.setAiFunction(aiFunction, profileId)),
  /** Whether cloud providers may see this account's mail (every account starts without). */
  setAccountCloud: (accountId: AccountId, allowed: boolean): Promise<AiSettingsInfo> =>
    call(() => aiCommands.setAiAccountCloud(accountId, allowed)),
  /** Only after the user read the explanation and agreed (or withdrew it). */
  setConsent: (profileId: string, granted: boolean): Promise<AiSettingsInfo> =>
    call(() => aiCommands.setAiConsent(profileId, granted)),
  /** The open-reply hints work without AI. */
  setOpenReplySettings: (settings: OpenReplySettings): Promise<AiSettingsInfo> =>
    call(() => aiCommands.setOpenReplySettings(settings)),
  /** Needs AI on; sends the key to the provider, no mail. */
  listModels: (profileId: string): Promise<AiModel[]> => call(() => aiCommands.listAiModels(profileId)),
  testConnection: (profileId: string): Promise<AiConnectionTest> => call(() => aiCommands.testAiConnection(profileId)),
  /** The request log (metadata only), newest first. `limit` 1..`AI_LIMITS.requestPage`. */
  listRequests: (before: AiRequestId | null, limit: number): Promise<AiRequestPage> =>
    call(() => aiCommands.listAiRequests(before, limit)),
  /** `false`: nothing was running under this id. The running call then rejects with `ai_cancelled`. */
  cancelRequest: (requestId: string): Promise<boolean> => infallible(() => aiCommands.cancelAiRequest(requestId)),
  summarizeThread: (requestId: string, messageId: MessageId): Promise<AiSummary> =>
    call(() => aiCommands.aiSummarizeThread(requestId, messageId)),
  suggestReply: (requestId: string, messageId: MessageId): Promise<AiReplySuggestion> =>
    call(() => aiCommands.aiSuggestReply(requestId, messageId)),
  /** A normal stage 5 reply draft with this text, kept open. No AI call. */
  openReplyDraft: (messageId: MessageId, text: string): Promise<Draft> =>
    call(() => aiCommands.aiOpenReplyDraft(messageId, text)),
  /** A preview; nothing in the draft changes until the UI applies it. */
  writingHelp: (requestId: string, passage: string, task: AiWritingTask): Promise<AiWritingResult> =>
    call(() => aiCommands.aiWritingHelp(requestId, passage, task)),
  /** `accountIds` null: every account (the unified view). */
  ask: (requestId: string, question: string, accountIds: AccountId[] | null): Promise<AiAnswer> =>
    call(() => aiCommands.aiAsk(requestId, question, accountIds)),
  /** Newest first, at most 50; empty while the hints are off. */
  listOpenReplyHints: (): Promise<OpenReplyHint[]> => call(() => aiCommands.listOpenReplyHints()),
  /** A dismissed hint never comes back. */
  dismissOpenReplyHint: (messageId: MessageId): Promise<void> =>
    done(call(() => aiCommands.dismissOpenReplyHint(messageId))),
};
