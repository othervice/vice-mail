// Stage 13 in the fake core: AI in the app, with the rules of the IPC contract (scratchpad
// stage13-ipc-contract.md, bindings). Tests pass in every value the UI shows; nothing here is
// shown without a test asking for it. `providerCalls` records every call that would have reached
// a provider (it passed the switch, the profile, the consent and the cloud permission): the
// "mock server" of the stage's acceptance tests.
import { emit } from "@tauri-apps/api/event";
import type {
  Account,
  AccountId,
  AiAnswer,
  AiClaim,
  AiConnectionTest,
  AiDeadline,
  AiFunction,
  AiModel,
  AiPreset,
  AiProfileInfo,
  AiProfileInput,
  AiReplySuggestion,
  AiRequestEntry,
  AiSettingsInfo,
  AiSummary,
  AiTransfer,
  AiTransferInput,
  AiWritingResult,
  AiWritingTask,
  CoreEvent,
  Draft,
  IpcError,
  MessageId,
  OpenReplyHint,
  OpenReplySettings,
  RenderToken,
  TextSegment,
} from "../../src/services/ipc";

// Decision 0028: results as tests write them. `segments` and `linkToken` may be left out; the
// fake fills them in as the core does for text without links (one text segment, no token).
type WithSegments<T> = Omit<T, "segments"> & { segments?: TextSegment[] };
type WithLinks<T> = Omit<T, "linkToken"> & { linkToken?: RenderToken | null };
export type ClaimFixture = WithSegments<AiClaim>;
export type SummaryFixture = WithLinks<Omit<AiSummary, "keyPoints" | "openQuestions" | "deadlines">> & {
  keyPoints: ClaimFixture[];
  openQuestions: ClaimFixture[];
  deadlines: WithSegments<AiDeadline>[];
};
export type ReplyFixture = WithLinks<WithSegments<AiReplySuggestion>>;
export type WritingFixture = WithLinks<WithSegments<AiWritingResult>>;
export type AnswerFixture = WithLinks<Omit<AiAnswer, "statements">> & { statements: ClaimFixture[] };

const textOnly = (text: string): TextSegment[] => (text === "" ? [] : [{ kind: "text", text }]);
const withSegments = <T extends { text: string; segments?: TextSegment[] }>(item: T): T & { segments: TextSegment[] } => ({
  ...item,
  segments: item.segments ?? textOnly(item.text),
});
const summaryOf = (s: SummaryFixture): AiSummary => ({
  ...s,
  keyPoints: s.keyPoints.map(withSegments),
  openQuestions: s.openQuestions.map(withSegments),
  deadlines: s.deadlines.map(withSegments),
  linkToken: s.linkToken ?? null,
});
const replyOf = (r: ReplyFixture): AiReplySuggestion => ({ ...withSegments(r), linkToken: r.linkToken ?? null });
const writingOf = (w: WritingFixture): AiWritingResult => ({ ...withSegments(w), linkToken: w.linkToken ?? null });
const answerOf = (a: AnswerFixture): AiAnswer => ({ ...a, statements: a.statements.map(withSegments), linkToken: a.linkToken ?? null });

export const AI_FUNCTIONS: readonly AiFunction[] = ["summary", "reply_suggestion", "writing_help", "search"];

/** Commands that reach a provider when everything allows it (the rest only read or change settings). */
export const AI_PROVIDER_COMMANDS = new Set([
  "list_ai_models",
  "test_ai_connection",
  "ai_summarize_thread",
  "ai_suggest_reply",
  "ai_writing_help",
  "ai_ask",
]);

export const AI_PRESETS: AiPreset[] = [
  { name: "Ollama", kind: "openai_compatible", baseUrl: "http://localhost:11434/v1", needsKey: false, locality: "this_device" },
  { name: "LM Studio", kind: "openai_compatible", baseUrl: "http://localhost:1234/v1", needsKey: false, locality: "this_device" },
  { name: "OpenAI", kind: "openai_compatible", baseUrl: "https://api.openai.com/v1", needsKey: true, locality: "internet" },
  { name: "Anthropic", kind: "anthropic", baseUrl: "https://api.anthropic.com", needsKey: true, locality: "internet" },
];

export function makeAiProfile(overrides: Partial<AiProfileInfo> = {}): AiProfileInfo {
  const locality = overrides.locality ?? "this_device";
  return {
    id: "0123456789abcdef0123456789abcdef",
    name: "Ollama",
    kind: "openai_compatible",
    baseUrl: "http://localhost:11434/v1",
    model: "llama3.2",
    structuredOutput: true,
    maxOutputTokens: null,
    hasKey: false,
    keyRemovedOnServerChange: false,
    host: "localhost:11434",
    locality,
    consentAt: null,
    needsConsent: locality === "internet",
    ...overrides,
  };
}

export const CLOUD_PROFILE_ID = "fedcba9876543210fedcba9876543210";

export function makeCloudProfile(overrides: Partial<AiProfileInfo> = {}): AiProfileInfo {
  return makeAiProfile({
    id: CLOUD_PROFILE_ID,
    name: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
    model: "gpt-5",
    hasKey: true,
    host: "api.openai.com:443",
    locality: "internet",
    ...overrides,
  });
}

export function makeTransfer(overrides: Partial<AiTransfer> = {}): AiTransfer {
  return {
    function: "summary",
    profileId: "0123456789abcdef0123456789abcdef",
    profileName: "Ollama",
    host: "localhost:11434",
    locality: "this_device",
    model: "llama3.2",
    mails: 3,
    accounts: [],
    bytes: 12_288,
    attachmentNames: 0,
    quotesRemoved: 0,
    trimming: { mailsLeftOut: 0, mailsShortened: 0, charsRemoved: 0 },
    ...overrides,
  };
}

export function makeRequestEntry(overrides: Partial<AiRequestEntry> = {}): AiRequestEntry {
  return {
    id: 1,
    at: Date.UTC(2026, 10, 23, 8, 0),
    function: "summary",
    profileName: "Ollama",
    host: "localhost:11434",
    locality: "this_device",
    model: "llama3.2",
    mails: 3,
    bytes: 12_288,
    outcome: "ok",
    error: null,
    durationMs: 2_400,
    ...overrides,
  };
}

export function makeHint(overrides: Partial<OpenReplyHint> = {}): OpenReplyHint {
  return {
    messageId: 1,
    accountId: "11111111-1111-4111-8111-111111111111",
    kind: "awaiting_reply",
    subject: "Budget 2027",
    counterpart: { name: "Grace Hopper", email: "grace@example.org" },
    date: Math.floor(Date.UTC(2026, 10, 18, 9, 0) / 1000),
    days: 5,
    ...overrides,
  };
}

export interface AiFakeOptions {
  enabled?: boolean;
  profiles?: AiProfileInfo[];
  /** Profile per function; missing ones are off. */
  functions?: Partial<Record<AiFunction, string | null>>;
  /** Accounts with cloud permission (all others have none). */
  cloudAccounts?: AccountId[];
  openReplies?: OpenReplySettings;
  hints?: OpenReplyHint[];
  log?: AiRequestEntry[];
  models?: Record<string, AiModel[]>;
  connection?: AiConnectionTest;
  /** Results of the functions (a function without one fails with `ai_failed`). */
  summary?: (messageId: MessageId) => SummaryFixture;
  reply?: (messageId: MessageId) => ReplyFixture;
  writing?: (passage: string, task: AiWritingTask) => WritingFixture;
  answer?: (question: string, accountIds: AccountId[] | null) => AnswerFixture;
  /** Decision 0028: what `aiPrepareTransfer` answers; default: the transfer the request would announce. */
  dryRun?: (input: AiTransferInput) => AiTransfer | Promise<never>;
  /** The account each message belongs to (for the cloud permission). Default: from the stored messages. */
  messageAccounts?: Record<number, AccountId>;
  /** Requests do not finish until `release()` (to see progress and cancel). */
  hold?: boolean;
}

export interface AiFakeState {
  readonly settings: AiSettingsInfo;
  /** Calls that would have reached a provider, with their arguments. */
  readonly providerCalls: readonly { cmd: string; args: Record<string, unknown> }[];
  /** Every key the UI sent (only `saveAiProfile` may carry one). */
  readonly keysReceived: readonly string[];
  readonly dismissed: readonly MessageId[];
  readonly cancelled: readonly string[];
  /** Decision 0028: every `aiPrepareTransfer` input (never a provider call). */
  readonly dryRuns: readonly AiTransferInput[];
  /** Finishes the held requests. */
  release: () => Promise<void>;
  setHints: (hints: OpenReplyHint[]) => void;
}

interface Deps {
  accounts: () => Account[];
  /** The account of a stored message, if the fake core has it. */
  accountOf: (messageId: MessageId) => AccountId | null;
  /** Stage 5: a reply draft from the compose fake (for "Open draft"). */
  createReply: (messageId: MessageId) => unknown;
}

const reject = (code: IpcError["code"], detail: string | null = null): Promise<never> =>
  // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
  Promise.reject({ code, message: `English fallback for ${code}`, detail } satisfies IpcError);

const event = (requestId: string, phase: Extract<CoreEvent, { type: "aiRequest" }>["phase"]) =>
  emit("core-event", { type: "aiRequest", requestId, phase } satisfies CoreEvent);

export function createAiFake(options: AiFakeOptions, deps: Deps) {
  let enabled = options.enabled ?? false;
  let profiles = [...(options.profiles ?? [])];
  const functions = new Map<AiFunction, string | null>(AI_FUNCTIONS.map((f) => [f, options.functions?.[f] ?? null]));
  const cloud = new Set(options.cloudAccounts ?? []);
  let openReplies: OpenReplySettings = options.openReplies ?? { enabled: true, days: 3 };
  let hints = [...(options.hints ?? [])];
  const providerCalls: { cmd: string; args: Record<string, unknown> }[] = [];
  const keysReceived: string[] = [];
  const dismissed: MessageId[] = [];
  const cancelled: string[] = [];
  const dryRuns: AiTransferInput[] = [];
  const running = new Map<string, { finish: () => void; cancel: () => void }>();
  let nextProfile = 1;

  const settings = (): AiSettingsInfo => ({
    enabled,
    profiles: profiles.map((p) => ({ ...p })),
    functions: AI_FUNCTIONS.map((f) => ({ function: f, profileId: functions.get(f) ?? null })),
    accounts: deps.accounts().map((a) => ({ accountId: a.id, cloudAllowed: cloud.has(a.id) })),
    presets: AI_PRESETS,
    openReplies,
  });
  const profileOf = (f: AiFunction) => profiles.find((p) => p.id === functions.get(f));
  const usable = (f: AiFunction) => enabled && Boolean(profileOf(f)?.model);

  function hostOf(url: URL): string {
    const port = url.port || (url.protocol === "https:" ? "443" : "80");
    return `${url.hostname}:${port}`;
  }

  function saveProfile(input: AiProfileInput): AiSettingsInfo | Promise<never> {
    let url: URL;
    try {
      url = new URL(input.baseUrl);
    } catch {
      return reject("invalid_argument", "base url");
    }
    if (!input.name.trim()) return reject("invalid_argument", "name");
    const local = url.hostname === "localhost" || url.hostname.startsWith("127.") || url.hostname.startsWith("192.168.");
    if (!local && url.protocol !== "https:") return reject("invalid_argument", "https");
    if (input.key.type === "set") keysReceived.push(input.key.key);
    const before = profiles.find((p) => p.id === input.id);
    if (input.id && !before) return reject("not_found");
    const host = hostOf(url);
    const locality = local ? (url.hostname === "localhost" || url.hostname.startsWith("127.") ? "this_device" : "local_network") : "internet";
    const sameHost = before?.host === host;
    const consentAt = locality === "internet" && sameHost ? (before.consentAt ?? null) : null;
    // V-02: a kept key stays only while scheme, host and port stay.
    const keyRemovedOnServerChange = input.key.type === "keep" && before !== undefined && before.hasKey && new URL(before.baseUrl).origin !== url.origin;
    const hasKey =
      input.key.type === "set" ? input.key.key !== "" : input.key.type === "remove" || keyRemovedOnServerChange ? false : (before?.hasKey ?? false);
    const next: AiProfileInfo = {
      id: input.id ?? `${String(nextProfile++).padStart(2, "0")}${"a".repeat(30)}`,
      name: input.name,
      kind: input.kind,
      baseUrl: input.baseUrl,
      model: input.model,
      structuredOutput: input.structuredOutput,
      maxOutputTokens: input.maxOutputTokens,
      hasKey,
      keyRemovedOnServerChange,
      host,
      locality,
      consentAt,
      needsConsent: locality === "internet" && consentAt === null,
    };
    profiles = before ? profiles.map((p) => (p.id === next.id ? next : p)) : [...profiles, next];
    return settings();
  }

  /** The checks of the core before a session opens, then the request with its events. */
  function request<T>(
    cmd: string,
    args: Record<string, unknown>,
    aiFunction: AiFunction,
    accountId: AccountId | null,
    result: (() => T) | undefined,
    transfer: (profile: AiProfileInfo) => AiTransfer,
  ): Promise<T> {
    if (!enabled) return reject("ai_off");
    const profile = profileOf(aiFunction);
    if (!profile?.model) return reject("ai_off");
    if (profile.needsConsent) return reject("ai_consent_required", profile.name);
    const requestId = args["requestId"] as string;
    if (running.has(requestId)) return reject("invalid_argument", "request id in use");
    if (profile.locality === "internet" && accountId !== null && !cloud.has(accountId)) {
      return event(requestId, { phase: "finished", outcome: "failed" }).then(() => reject("ai_not_permitted"));
    }
    providerCalls.push({ cmd, args });
    return new Promise<T>((resolve, rejectPromise) => {
      const finish = () => {
        running.delete(requestId);
        void (async () => {
          await event(requestId, { phase: "receiving", chars: 512 });
          if (!result) {
            await event(requestId, { phase: "finished", outcome: "failed" });
            // Tauri rejects with the serialised error value, not an Error.
            // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
            rejectPromise({ code: "ai_failed", message: "no result", detail: null } satisfies IpcError);
            return;
          }
          await event(requestId, { phase: "finished", outcome: "ok" });
          resolve(result());
        })();
      };
      const cancel = () => {
        running.delete(requestId);
        cancelled.push(requestId);
        void event(requestId, { phase: "finished", outcome: "cancelled" }).then(() => {
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          rejectPromise({ code: "ai_cancelled", message: "cancelled", detail: null } satisfies IpcError);
        });
      };
      running.set(requestId, { finish, cancel });
      void event(requestId, { phase: "sending", transfer: transfer(profile) }).then(() => {
        if (!options.hold) finish();
      });
    });
  }

  const transferFor = (aiFunction: AiFunction, mails: number) => (profile: AiProfileInfo) =>
    makeTransfer({
      function: aiFunction,
      profileId: profile.id,
      profileName: profile.name,
      host: profile.host,
      locality: profile.locality,
      model: profile.model,
      mails,
    });
  const accountOf = (messageId: MessageId) =>
    options.messageAccounts?.[messageId] ?? deps.accountOf(messageId) ?? deps.accounts()[0]?.id ?? null;

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "get_ai_settings":
        return settings();
      case "get_ai_availability":
        return {
          enabled,
          summary: usable("summary"),
          replySuggestion: usable("reply_suggestion"),
          writingHelp: usable("writing_help"),
          search: usable("search"),
        };
      case "set_ai_enabled":
        enabled = args["enabled"] as boolean;
        if (!enabled) for (const run of [...running.values()]) run.cancel();
        return settings();
      case "save_ai_profile":
        return saveProfile(args["profile"] as AiProfileInput);
      case "delete_ai_profile": {
        const id = args["profileId"] as string;
        if (!profiles.some((p) => p.id === id)) return reject("not_found");
        profiles = profiles.filter((p) => p.id !== id);
        for (const f of AI_FUNCTIONS) if (functions.get(f) === id) functions.set(f, null);
        return settings();
      }
      case "set_ai_function": {
        const id = args["profileId"] as string | null;
        if (id !== null && !profiles.some((p) => p.id === id)) return reject("not_found");
        functions.set(args["aiFunction"] as AiFunction, id);
        return settings();
      }
      case "set_ai_account_cloud": {
        const accountId = args["accountId"] as string;
        if (args["allowed"] as boolean) cloud.add(accountId);
        else cloud.delete(accountId);
        return settings();
      }
      case "set_ai_consent": {
        const id = args["profileId"] as string;
        const granted = args["granted"] as boolean;
        profiles = profiles.map((p) =>
          p.id === id && p.locality === "internet"
            ? { ...p, consentAt: granted ? Date.UTC(2026, 10, 23, 8, 30) : null, needsConsent: !granted }
            : p,
        );
        return settings();
      }
      case "set_open_reply_settings": {
        const next = args["settings"] as OpenReplySettings;
        if (next.days < 1 || next.days > 30) return reject("invalid_argument", "days");
        openReplies = next;
        return settings();
      }
      case "list_ai_models": {
        if (!enabled) return reject("ai_off");
        providerCalls.push({ cmd, args });
        return options.models?.[args["profileId"] as string] ?? [];
      }
      case "test_ai_connection": {
        if (!enabled) return reject("ai_off");
        providerCalls.push({ cmd, args });
        return options.connection ?? { models: 2, modelListed: true, chatOk: true, chatError: null };
      }
      case "list_ai_requests": {
        const before = args["before"] as number | null;
        const limit = args["limit"] as number;
        const older = (options.log ?? []).filter((e) => before === null || e.id < before);
        const entries = older.slice(0, limit);
        const last = entries[entries.length - 1];
        return { entries, nextBefore: older.length > limit && last ? last.id : null };
      }
      case "cancel_ai_request": {
        const run = running.get(args["requestId"] as string);
        run?.cancel();
        return Boolean(run);
      }
      case "ai_summarize_thread": {
        const messageId = args["messageId"] as number;
        const summary = options.summary;
        return request(cmd, args, "summary", accountOf(messageId), summary && (() => summaryOf(summary(messageId))), transferFor("summary", 3));
      }
      case "ai_suggest_reply": {
        const messageId = args["messageId"] as number;
        const reply = options.reply;
        return request(cmd, args, "reply_suggestion", accountOf(messageId), reply && (() => replyOf(reply(messageId))), transferFor("reply_suggestion", 3));
      }
      case "ai_writing_help": {
        const passage = args["passage"] as string;
        if (!passage.trim() || passage.length > 12_000) return reject("invalid_argument", "passage");
        const writing = options.writing;
        const task = args["task"] as AiWritingTask;
        return request(cmd, args, "writing_help", null, writing && (() => writingOf(writing(passage, task))), transferFor("writing_help", 0));
      }
      case "ai_ask": {
        const question = args["question"] as string;
        const accountIds = args["accountIds"] as AccountId[] | null;
        if (!question.trim()) return reject("invalid_argument", "question");
        const answer = options.answer;
        return request(cmd, args, "search", null, answer && (() => answerOf(answer(question, accountIds))), transferFor("search", 0));
      }
      case "ai_prepare_transfer": {
        // Decision 0028: the checks of the core before preparing, but no consent and no request.
        const input = args["input"] as AiTransferInput;
        dryRuns.push(input);
        if (!enabled) return reject("ai_off");
        const profile = profileOf(input.function);
        if (!profile?.model) return reject("ai_off");
        const accountId = "messageId" in input ? accountOf(input.messageId) : null;
        if (profile.locality === "internet" && accountId !== null && !cloud.has(accountId)) return reject("ai_not_permitted");
        if (options.dryRun) return options.dryRun(input);
        const mails = input.function === "summary" || input.function === "reply_suggestion" ? 3 : 0;
        return transferFor(input.function, mails)(profile);
      }
      case "ai_open_reply_draft": {
        const messageId = args["messageId"] as number;
        const text = args["text"] as string;
        return Promise.resolve(deps.createReply(messageId)).then((draft) => ({ ...(draft as Draft), format: "text", bodyText: text }));
      }
      case "list_open_reply_hints":
        return openReplies.enabled ? hints.filter((h) => !dismissed.includes(h.messageId)) : [];
      case "dismiss_open_reply_hint": {
        const messageId = args["messageId"] as number;
        if (!hints.some((h) => h.messageId === messageId)) return reject("not_found");
        dismissed.push(messageId);
        return null;
      }
      default:
        return undefined;
    }
  }

  const state: AiFakeState = {
    get settings() {
      return settings();
    },
    providerCalls,
    keysReceived,
    dismissed,
    cancelled,
    dryRuns,
    release: async () => {
      for (const run of [...running.values()]) run.finish();
      await Promise.resolve();
    },
    setHints: (next) => {
      hints = [...next];
    },
  };
  return { handle, state };
}
