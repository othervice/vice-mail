// Stages 12 and 11 in the fake core: AI over MCP and the settings sync, with the rules of the
// IPC contracts (scratchpad stage12/stage11 contracts, bindings). Tests pass in every value the
// UI shows; nothing here is shown without a test asking for it.
import {
  MIN_SYNC_PASSPHRASE_CHARS,
  SETTINGS_SYNC_FOLDER,
  type Account,
  type AccountId,
  type IpcError,
  type McpAccessEntry,
  type McpAccountAccess,
  type McpClientConfig,
  type McpClientCreated,
  type McpClientInfo,
  type McpClientKind,
  type McpConnectionTest,
  type McpSettingsInfo,
  type McpSetup,
  type McpStatus,
  type PasswordRequest,
  type SettingsSyncPreview,
  type SettingsSyncStatus,
  type SyncDevice,
} from "../../src/services/ipc";

export const MCP_HELPER_PATH = "/Applications/Vice Mail.app/Contents/MacOS/mail-mcp";
export const MCP_ENDPOINT = "/Users/ada/Library/Application Support/vice-mail/mcp/mail-mcp.sock";
/** `MCP_ENDPOINT` as the core shows it, from the home directory. */
export const MCP_ENDPOINT_SHOWN = "~/Library/Application Support/vice-mail/mcp/mail-mcp.sock";
const KINDS: readonly McpClientKind[] = ["claude_desktop", "claude_code", "cursor", "lm_studio"];

/** The four configurations as `mail_mcp::config` builds them, with `token` filled in. */
export function mcpConfigs(token: string): McpClientConfig[] {
  const env = { MAIL_MCP_ENDPOINT: MCP_ENDPOINT, MAIL_MCP_TOKEN: token };
  const json = JSON.stringify({ mcpServers: { "vice-mail": { command: MCP_HELPER_PATH, args: [], env } } }, null, 2);
  return KINDS.map((kind) => ({
    kind,
    location:
      kind === "claude_code"
        ? "terminal"
        : kind === "lm_studio"
          ? "LM Studio: Program > Install > Edit mcp.json"
          : kind === "cursor"
            ? "~/.cursor/mcp.json"
            : "~/Library/Application Support/Claude/claude_desktop_config.json",
    content:
      kind === "claude_code"
        ? `claude mcp add --transport stdio --scope user --env MAIL_MCP_TOKEN=${token} vice-mail -- '${MCP_HELPER_PATH}'`
        : json,
    // Finding F5: only Claude Code gets the command that names a key file instead of the key.
    tokenFileContent:
      kind === "claude_code"
        ? `claude mcp add --transport stdio --scope user --env "MAIL_MCP_TOKEN_FILE=$HOME/.mail-mcp-token" vice-mail -- '${MCP_HELPER_PATH}'`
        : null,
    source: `https://docs.example.test/${kind}`,
  }));
}

export function makeMcpClient(overrides: Partial<McpClientInfo> = {}): McpClientInfo {
  return {
    clientId: "0123456789abcdef0123456789abcdef",
    name: "Claude Desktop",
    createdAt: Date.UTC(2026, 10, 20, 9, 0),
    revokedAt: null,
    lastUsedAt: null,
    ...overrides,
  };
}

export function makeAccessEntry(overrides: Partial<McpAccessEntry> & Pick<McpAccessEntry, "id">): McpAccessEntry {
  return {
    at: Date.UTC(2026, 10, 22, 10, 0) + overrides.id * 1000,
    clientId: "0123456789abcdef0123456789abcdef",
    clientName: "Claude Desktop",
    tool: "search_messages",
    messages: 3,
    outcome: "ok",
    refusal: null,
    ...overrides,
  };
}

export const SYNC_OFF: SettingsSyncStatus = {
  state: "off",
  accountId: null,
  folder: null,
  lastSyncAt: null,
  lastError: null,
  pendingChanges: 0,
  rejectedEntries: 0,
  devices: [],
  questions: [],
  accountsNeedingPassword: [],
  detectedAccounts: [],
  serverChanges: [],
  passwordTransfer: null,
  passwordRequests: [],
};

export function makeDevice(overrides: Partial<SyncDevice> = {}): SyncDevice {
  return {
    id: "device-this",
    name: "Mac",
    system: "macOS (aarch64)",
    lastSeenAt: Date.UTC(2026, 10, 22, 8, 0),
    current: true,
    ...overrides,
  };
}

/** Stage 22: the request of another device as a pass shows it. */
export function makePasswordRequest(overrides: Partial<PasswordRequest> = {}): PasswordRequest {
  return {
    requestId: "0123456789abcdef0123456789abcdef",
    deviceName: "New Mac",
    expiresAt: Date.UTC(2026, 10, 26, 10, 15),
    state: "open",
    code: null,
    accounts: [],
    ...overrides,
  };
}

/** Stage 22: how long a request runs. */
export const TRANSFER_LIFETIME_MS = 15 * 60 * 1000;

export function syncStatus(overrides: Partial<SettingsSyncStatus> = {}): SettingsSyncStatus {
  return { ...SYNC_OFF, ...overrides };
}

export interface McpSyncFakeOptions {
  mcp?: {
    enabled?: boolean;
    shares?: Record<AccountId, McpAccountAccess>;
    /** Newest first, as the core lists them. */
    clients?: McpClientInfo[];
    status?: Partial<McpStatus>;
    /** The access log, newest first. */
    access?: McpAccessEntry[];
    setup?: McpSetup;
    test?: McpConnectionTest;
  };
  sync?: {
    status?: SettingsSyncStatus;
    /** The passphrase of the sync folder that exists in the mailbox. */
    passphrase?: string;
    /** Accounts whose mailbox already holds a sync folder (`enable` answers `sync_already_set_up`). */
    folderAccounts?: AccountId[];
    preview?: SettingsSyncPreview;
    /** What the status looks like right after `join`. */
    joined?: Partial<SettingsSyncStatus>;
  };
}

export interface McpSyncFakeState {
  readonly mcpEnabled: boolean;
  readonly shares: ReadonlyMap<AccountId, McpAccountAccess>;
  readonly clients: readonly McpClientInfo[];
  readonly syncStatus: SettingsSyncStatus;
  readonly syncPassphrase: string | undefined;
  /** Replaces the sync status, as a pass would; the test then emits `settingsSyncChanged`. */
  setSyncStatus: (status: SettingsSyncStatus) => void;
  /** New access log entries (the test then emits `mcpAccess`). */
  addAccess: (entries: readonly McpAccessEntry[]) => void;
}

interface Deps {
  accounts: () => Account[];
  /** Calls the fake core recorded (for `update_password`, which the base fake answers). */
  calls: () => readonly { cmd: string; args: unknown }[];
}

function reject(code: IpcError["code"], detail: string | null = null): Promise<never> {
  // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
  return Promise.reject({ code, message: `English fallback for ${code}`, detail } satisfies IpcError);
}

export function createMcpSyncFake(options: McpSyncFakeOptions, deps: Deps) {
  // ---- MCP ----
  let enabled = options.mcp?.enabled ?? false;
  const shares = new Map<AccountId, McpAccountAccess>(Object.entries(options.mcp?.shares ?? {}));
  let clients = [...(options.mcp?.clients ?? [])];
  let access = [...(options.mcp?.access ?? [])];
  let nextClient = 1;

  const status = (): McpStatus => ({
    state: enabled ? "listening" : "stopped",
    endpoint: enabled ? MCP_ENDPOINT : null,
    endpointDisplay: enabled ? MCP_ENDPOINT_SHOWN : null,
    error: null,
    helper: { path: MCP_HELPER_PATH, displayPath: MCP_HELPER_PATH, found: true, unstableLocation: false },
    ...options.mcp?.status,
    enabled,
  });

  const settings = (): McpSettingsInfo => ({
    enabled,
    accounts: deps.accounts().map((account) => ({ accountId: account.id, access: shares.get(account.id) ?? "hidden" })),
    clients: clients.map((client) => ({ ...client })),
    status: status(),
  });

  // ---- Settings sync ----
  let sync: SettingsSyncStatus = options.sync?.status ?? SYNC_OFF;
  let passphrase = options.sync?.passphrase;
  let previewed: AccountId | null = null;
  // A detected folder is a folder that exists.
  const folderAccounts = new Set([...(options.sync?.folderAccounts ?? []), ...sync.detectedAccounts]);

  const passwordsGiven = () =>
    new Set(deps.calls().flatMap((call) => (call.cmd === "update_password" ? [(call.args as { accountId: string }).accountId] : [])));
  const syncNow = (): SettingsSyncStatus => {
    const given = passwordsGiven();
    return { ...sync, accountsNeedingPassword: sync.accountsNeedingPassword.filter((id) => !given.has(id)) };
  };
  const tooShort = (text: unknown) => typeof text !== "string" || [...text].length < MIN_SYNC_PASSPHRASE_CHARS;

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      // ---- Stage 12 ----
      case "get_mcp_settings":
        return settings();
      case "set_mcp_enabled":
        enabled = args["enabled"] as boolean;
        return settings();
      case "set_mcp_account_access": {
        const accountId = args["accountId"] as string;
        if (!deps.accounts().some((account) => account.id === accountId)) return reject("not_found");
        shares.set(accountId, args["access"] as McpAccountAccess);
        return settings();
      }
      case "create_mcp_client": {
        const name = args["name"] as string;
        if ([...name].length < 1 || [...name].length > 100) return reject("invalid_argument", "name");
        const clientId = (nextClient++).toString(16).padStart(32, "a");
        const client = makeMcpClient({ clientId, name, createdAt: Date.now() });
        clients = [client, ...clients];
        const token = `mcp1_${clientId}_s3cr3tkey`;
        return { client, token, configs: mcpConfigs(token) } satisfies McpClientCreated;
      }
      case "revoke_mcp_client": {
        const client = clients.find((c) => c.clientId === args["clientId"]);
        if (!client) return reject("not_found");
        clients = clients.map((c) => (c === client && c.revokedAt === null ? { ...c, revokedAt: Date.now() } : c));
        return settings();
      }
      case "list_mcp_clients":
        return settings().clients;
      case "get_mcp_setup":
        return (
          options.mcp?.setup ?? {
            helper: status().helper,
            endpoint: status().endpoint,
            configs: mcpConfigs("<token>"),
          }
        );
      case "list_mcp_access": {
        const before = args["before"] as number | null;
        const limit = args["limit"] as number;
        if (limit < 1 || limit > 500) return reject("invalid_argument", "limit");
        const after = access.filter((entry) => before === null || entry.id < before);
        const entries = after.slice(0, limit);
        const last = entries[entries.length - 1];
        return { entries, nextCursor: after.length > limit && last ? last.id : null };
      }
      case "get_mcp_status":
        return status();
      case "test_mcp_connection":
        return (
          options.mcp?.test ?? {
            ok: enabled,
            problem: enabled ? null : "disabled",
            refusal: null,
            helperPath: MCP_HELPER_PATH,
            protocolVersion: enabled ? "2025-06-18" : null,
            tools: enabled ? ["list_accounts", "search_messages"] : [],
            sharedAccounts: [...shares.values()].filter((level) => level !== "hidden").length,
            detail: null,
            durationMs: 120,
          }
        );

      // ---- Stage 11 ----
      case "get_settings_sync_status":
        return syncNow();
      case "enable_settings_sync": {
        const accountId = args["accountId"] as string;
        if (tooShort(args["passphrase"])) return reject("invalid_argument", "passphrase");
        if (!deps.accounts().some((account) => account.id === accountId)) return reject("not_found");
        if (folderAccounts.has(accountId)) return reject("sync_already_set_up");
        passphrase = args["passphrase"] as string;
        folderAccounts.add(accountId);
        sync = {
          ...SYNC_OFF,
          state: "active",
          accountId,
          folder: SETTINGS_SYNC_FOLDER,
          lastSyncAt: Date.now(),
          devices: [makeDevice()],
        };
        return syncNow();
      }
      case "preview_settings_sync_join": {
        const accountId = args["accountId"] as string;
        if (!folderAccounts.has(accountId) && sync.accountId !== accountId) return reject("sync_not_set_up");
        if (args["passphrase"] !== passphrase) return reject("sync_wrong_passphrase");
        previewed = accountId;
        return options.sync?.preview ?? { kinds: [], newAccounts: [], uploads: 0, devices: [] };
      }
      case "join_settings_sync": {
        if (previewed === null) return reject("invalid_argument", "no preview");
        sync = {
          ...SYNC_OFF,
          state: "active",
          accountId: previewed,
          folder: SETTINGS_SYNC_FOLDER,
          lastSyncAt: Date.now(),
          devices: [makeDevice()],
          ...options.sync?.joined,
        };
        previewed = null;
        return syncNow();
      }
      case "sync_settings_now":
        if (sync.state === "off") return reject("sync_off");
        if (sync.state === "needsPassphrase") return reject(sync.lastError?.code ?? "sync_key_missing");
        sync = { ...sync, state: "active", lastError: null, lastSyncAt: Date.now() };
        return syncNow();
      case "resolve_sync_question": {
        const accountId = args["accountId"] as string;
        if (!sync.questions.some((question) => question.accountId === accountId)) return reject("not_found");
        sync = { ...sync, questions: sync.questions.filter((question) => question.accountId !== accountId) };
        if (args["remove"] === true) {
          const list = deps.accounts();
          const index = list.findIndex((account) => account.id === accountId);
          if (index >= 0) list.splice(index, 1);
        }
        return syncNow();
      }
      // Decision 0040: taking the servers switches writing off and asks for the password again.
      case "resolve_server_change": {
        const accountId = args["accountId"] as string;
        const change = sync.serverChanges.find((c) => c.accountId === accountId);
        if (!change) return reject("not_found");
        sync = { ...sync, serverChanges: sync.serverChanges.filter((c) => c.accountId !== accountId) };
        if (args["apply"] === true) {
          const list = deps.accounts();
          const index = list.findIndex((account) => account.id === accountId);
          const account = list[index];
          if (account) list[index] = { ...account, imap: change.imap, smtp: change.smtp, writesEnabled: false };
          sync = { ...sync, accountsNeedingPassword: [...sync.accountsNeedingPassword, accountId] };
        }
        return syncNow();
      }
      case "change_settings_sync_passphrase":
        if (tooShort(args["passphrase"])) return reject("invalid_argument", "passphrase");
        if (sync.state === "off") return reject("sync_off");
        passphrase = args["passphrase"] as string;
        return null;
      case "reset_settings_sync":
        if (tooShort(args["passphrase"])) return reject("invalid_argument", "passphrase");
        if (sync.state === "off") return reject("sync_off");
        passphrase = args["passphrase"] as string;
        sync = { ...sync, state: "active", lastError: null, rejectedEntries: 0, lastSyncAt: Date.now() };
        return syncNow();
      case "disable_settings_sync":
        sync = { ...SYNC_OFF, detectedAccounts: sync.accountId ? [sync.accountId] : [] };
        return syncNow();
      // ---- Stage 22 (decision 0076): passwords from another device ----
      case "start_password_transfer": {
        if (sync.state === "off") return reject("sync_off");
        const waiting = syncNow().accountsNeedingPassword;
        if (waiting.length === 0) return reject("invalid_argument", "no account waits for its password");
        sync = {
          ...sync,
          passwordTransfer: {
            state: "waiting",
            code: null,
            otherDevice: null,
            expiresAt: Date.now() + TRANSFER_LIFETIME_MS,
            accounts: waiting,
            stored: [],
            skipped: [],
          },
        };
        return syncNow();
      }
      case "cancel_password_transfer":
        sync = { ...sync, passwordTransfer: null };
        return syncNow();
      case "open_password_request":
      case "give_passwords":
      case "decline_password_request": {
        const requestId = args["requestId"] as string;
        const request = sync.passwordRequests.find((r) => r.requestId === requestId);
        if (!request) return reject("not_found");
        let state = request.state;
        if (cmd === "open_password_request") state = request.state === "open" ? "connecting" : request.state;
        if (cmd === "decline_password_request") state = "declined";
        if (cmd === "give_passwords") {
          if (request.state !== "confirm") return reject("invalid_argument", "compare the codes first");
          const ids = args["accountIds"] as string[];
          if (!ids.some((id) => request.accounts.some((a) => a.accountId === id && a.state === "ready"))) {
            return reject("invalid_argument", "nothing to give");
          }
          state = "given";
        }
        sync = {
          ...sync,
          passwordRequests: sync.passwordRequests.map((r) => (r === request ? { ...r, state, code: state === "confirm" ? r.code : null } : r)),
        };
        return syncNow();
      }
      case "set_sync_device_name": {
        const name = args["name"] as string;
        if ([...name].length < 1 || [...name].length > 64) return reject("invalid_argument", "name");
        sync = { ...sync, devices: sync.devices.map((device) => (device.current ? { ...device, name } : device)) };
        return syncNow();
      }
      default:
        return undefined;
    }
  }

  const state: McpSyncFakeState = {
    get mcpEnabled() {
      return enabled;
    },
    get shares() {
      return shares;
    },
    get clients() {
      return clients;
    },
    get syncStatus() {
      return syncNow();
    },
    get syncPassphrase() {
      return passphrase;
    },
    setSyncStatus: (next) => {
      sync = next;
    },
    addAccess: (entries) => {
      access = [...[...entries].sort((a, b) => b.id - a.id), ...access];
    },
  };

  return { handle, state };
}
