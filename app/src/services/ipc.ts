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
  /** Opens a link in the system browser. Only after the user confirmed the target. */
  openLink: (renderToken: RenderToken, index: number): Promise<void> =>
    done(call(() => commands.openLink(renderToken, index))),
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
