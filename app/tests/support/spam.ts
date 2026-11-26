import type {
  MessageId,
  SpamFilterSettings,
  SpamFilterStatus,
  SpamLearning,
  SpamVerdict,
} from "../../src/services/ipc";

/** Stage 22 (decision 0077): what the fake core answers for the learning spam filter. */
export interface SpamFakeOptions {
  /** Default: off, nothing learned. */
  status?: Partial<SpamFilterStatus>;
  /** The filter's verdicts by message id (default: none). */
  verdicts?: Record<MessageId, Omit<SpamVerdict, "messageId">>;
}

export interface SpamFakeState {
  readonly status: SpamFilterStatus;
  readonly verdicts: ReadonlyMap<MessageId, SpamVerdict>;
  /** `markSpamLocally` calls: ids and the verdict. */
  readonly local: { messageIds: MessageId[]; spam: boolean }[];
  /** Sets what a later `getSpamFilter` answers (a learning run, an event of the core). */
  setLearning: (learning: SpamLearning | null) => void;
}

export const SPAM_OFF: SpamFilterStatus = {
  settings: { enabled: false, excludedAccounts: [] },
  learnedSpam: 0,
  learnedHam: 0,
  tokens: 0,
  ready: false,
  minSpam: 50,
  minHam: 200,
  learning: null,
};

export function createSpamFake(options: SpamFakeOptions, deps: { reject: (code: "invalid_argument") => unknown }) {
  let status: SpamFilterStatus = { ...SPAM_OFF, ...options.status };
  const verdicts = new Map<MessageId, SpamVerdict>(
    Object.entries(options.verdicts ?? {}).map(([id, verdict]) => [Number(id), { ...verdict, messageId: Number(id) }]),
  );
  const local: { messageIds: MessageId[]; spam: boolean }[] = [];

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "get_spam_filter":
        return { ...status };
      case "set_spam_filter": {
        const settings = args["settings"] as SpamFilterSettings;
        status = { ...status, settings: { ...settings, excludedAccounts: [...(settings.excludedAccounts ?? [])] } };
        if (!settings.enabled && status.learning?.state === "running") {
          status = { ...status, learning: { ...status.learning, state: "cancelled" } };
        }
        return { ...status };
      }
      case "learn_spam_from_mail":
        if (!status.settings.enabled) return deps.reject("invalid_argument");
        status = {
          ...status,
          learning: { state: "running", done: 0, total: 1500, spam: 0, ham: 0, skipped: 0, error: null },
        };
        return { ...status };
      case "cancel_spam_learning":
        if (status.learning?.state === "running") status = { ...status, learning: { ...status.learning, state: "cancelled" } };
        return null;
      case "forget_spam_filter":
        verdicts.clear();
        status = { ...status, learnedSpam: 0, learnedHam: 0, tokens: 0, ready: false };
        return { ...status };
      case "get_spam_verdict":
        return verdicts.get(args["messageId"] as MessageId) ?? null;
      case "mark_spam_locally": {
        const messageIds = args["messageIds"] as MessageId[];
        const spam = args["spam"] === true;
        local.push({ messageIds, spam });
        let changed = 0;
        for (const id of messageIds) {
          const had = verdicts.get(id);
          if (spam) {
            verdicts.set(id, { messageId: id, mailboxId: had?.mailboxId ?? null, mark: "suspected", score: 1, at: Date.now(), canMove: false });
            changed += 1;
          } else if (had) {
            verdicts.delete(id);
            changed += 1;
          }
        }
        status = spam ? { ...status, learnedSpam: status.learnedSpam + messageIds.length } : { ...status, learnedHam: status.learnedHam + messageIds.length };
        return changed;
      }
      default:
        return undefined;
    }
  }

  const state: SpamFakeState = {
    get status() {
      return status;
    },
    verdicts,
    local,
    setLearning: (learning) => {
      status = { ...status, learning };
    },
  };
  return { handle, state };
}
