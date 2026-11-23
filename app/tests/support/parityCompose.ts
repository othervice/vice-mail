// Stage 16 part of the fake core (decision 0033): read receipts (RFC 8098), with the command
// names and argument shapes of the IPC contract. The fake never sends a receipt on its own: only
// `send_read_receipt` records one, so a test can prove that nothing went out without a click.
import type {
  IpcError,
  OutgoingMail,
  QuoteLanguage,
  ReadReceiptInfo,
  ReadReceiptPolicy,
} from "../../src/services/ipc";

export interface ParityComposeFakeOptions {
  /** What `getReadReceipt` answers, by message id. Default: no request, nothing received. */
  receipts?: Record<number, ReadReceiptInfo>;
}

export interface ParityComposeFakeState {
  /** Every receipt that `sendReadReceipt` put in the outbox, in order. */
  readonly sentReceipts: readonly { messageId: number; language: QuoteLanguage; requestId: string; mail: OutgoingMail }[];
  /** Messages the user answered "Don't send" for. */
  readonly declined: readonly number[];
}

interface Deps {
  policy: () => ReadReceiptPolicy;
  accountOf: (messageId: number) => { id: string; writesEnabled: boolean } | null;
  reject: (code: IpcError["code"], detail?: string | null) => Promise<never>;
}

export const NO_RECEIPT: ReadReceiptInfo = { request: null, requested: false, received: [], receiptFor: null };

export function makeReceiptInfo(overrides: Partial<ReadReceiptInfo> = {}): ReadReceiptInfo {
  return { ...NO_RECEIPT, ...overrides };
}

export function createParityComposeFake(options: ParityComposeFakeOptions, deps: Deps) {
  const receipts = new Map<number, ReadReceiptInfo>(
    Object.entries(options.receipts ?? {}).map(([id, info]) => [Number(id), info]),
  );
  const sentReceipts: ParityComposeFakeState["sentReceipts"][number][] = [];
  const declined: number[] = [];
  let nextOperation = 7000;

  /** The answer as the core shows it: `off` while the setting is "never". */
  const info = (messageId: number): ReadReceiptInfo => {
    const stored = receipts.get(messageId) ?? NO_RECEIPT;
    if (stored.request && deps.policy() === "never" && stored.request.answer === "ask") {
      return { ...stored, request: { ...stored.request, answer: "off" } };
    }
    return stored;
  };

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "get_read_receipt":
        return info(args["messageId"] as number);
      case "send_read_receipt": {
        const messageId = args["messageId"] as number;
        const requestId = args["requestId"] as string;
        const known = sentReceipts.find((r) => r.messageId === messageId);
        if (known) return known.mail;
        const stored = receipts.get(messageId);
        if (deps.policy() === "never") return deps.reject("invalid_argument", "read receipts are off");
        if (!stored?.request) return deps.reject("invalid_argument", "no receipt requested");
        const account = deps.accountOf(messageId);
        if (!account) return deps.reject("not_found");
        if (!account.writesEnabled) return deps.reject("read_only");
        const now = Date.now();
        const mail: OutgoingMail = {
          operationId: nextOperation++,
          accountId: account.id,
          draftId: null,
          subject: "Read receipt",
          to: [{ name: null, email: stored.request.to }],
          recipientCount: 1,
          state: "queued",
          scheduled: false,
          sendAt: now,
          error: null,
          attempts: 0,
          notice: null,
          archive: null,
          readReceipt: true,
          createdAt: now,
          updatedAt: now,
        };
        sentReceipts.push({ messageId, language: args["language"] as QuoteLanguage, requestId, mail });
        receipts.set(messageId, { ...stored, request: { ...stored.request, answer: "queued" } });
        return mail;
      }
      case "decline_read_receipt": {
        const messageId = args["messageId"] as number;
        const stored = receipts.get(messageId);
        if (!stored?.request) return deps.reject("invalid_argument", "no receipt requested");
        if (stored.request.answer === "queued" || stored.request.answer === "sent") return info(messageId);
        declined.push(messageId);
        receipts.set(messageId, { ...stored, request: { ...stored.request, answer: "declined" } });
        return info(messageId);
      }
      default:
        return undefined;
    }
  }

  const state: ParityComposeFakeState = { sentReceipts, declined };
  return { handle, state };
}
