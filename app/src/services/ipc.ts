// The only module that talks to the core. It is also the only module allowed to import the
// generated bindings and `@tauri-apps/api` (enforced by ESLint `no-restricted-imports`).
//
// Command names, argument names, result types and the event name come from the bindings that
// tauri-specta generates from Rust, so a rename on the Rust side breaks the TypeScript build.
import {
  commands,
  events,
  type Account,
  type AccountColor,
  type AccountId,
  type AccountSettings,
  type AccountState,
  type AccountStatus,
  type Address,
  type AppInfo,
  type Capabilities,
  type ConnectionTestResult,
  type CoreEvent,
  type DiscoveryResult,
  type DiscoverySource,
  type ErrorCode,
  type IpcError,
  type LanguagePreference,
  type ListCursor,
  type Mailbox,
  type MailboxId,
  type MailboxRole,
  type MessageId,
  type MessageListItem,
  type MessagePage,
  type Platform,
  type Preferences,
  type ProviderHint,
  type RoleSource,
  type Security,
  type ServerSettings,
  type SyncProgress,
  type ThemePreference,
} from "../generated/bindings";

export type {
  Account,
  AccountColor,
  AccountId,
  AccountSettings,
  AccountState,
  AccountStatus,
  Address,
  AppInfo,
  Capabilities,
  ConnectionTestResult,
  CoreEvent,
  DiscoveryResult,
  DiscoverySource,
  ErrorCode,
  IpcError,
  LanguagePreference,
  ListCursor,
  Mailbox,
  MailboxId,
  MailboxRole,
  MessageId,
  MessageListItem,
  MessagePage,
  Platform,
  Preferences,
  ProviderHint,
  RoleSource,
  Security,
  ServerSettings,
  SyncProgress,
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
