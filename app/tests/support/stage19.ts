// Stage 19 (decision 0055): the fake core's source window, all headers and redirecting, with the
// contract's rules (one window per message, `read_only` without writes, the same request id once,
// `cancelRedirect` only while it waits).
import type {
  Account,
  AccountId,
  Address,
  HeaderField,
  IpcError,
  MessageId,
  MessageListItem,
  MessageSource,
  OutgoingMail,
  RedirectRequest,
} from "../../src/services/ipc";

export interface Stage19FakeOptions {
  /** `getMessageSource` answers by message id; default: a small text made from the list row. */
  sources?: Record<MessageId, MessageSource | IpcError>;
  /** `getMessageHeaders` answers by message id; default: From, To, Subject of the row. */
  headers?: Record<MessageId, HeaderField[] | IpcError>;
  /** The undo delay of redirects in seconds (default 10). */
  redirectDelaySeconds?: number;
}

export interface Stage19FakeState {
  /** Source windows opened, as `<accountId>/<messageId>`, once each. */
  readonly sourceWindows: readonly string[];
  readonly redirects: readonly RedirectRequest[];
  readonly cancelled: readonly number[];
}

interface Deps {
  messages: () => MessageListItem[];
  accounts: () => Account[];
  reject: (code: IpcError["code"], detail?: string | null) => Promise<never>;
}

function isError<T extends object>(value: T | IpcError): value is IpcError {
  return "code" in value && "message" in value;
}

export function createStage19Fake(options: Stage19FakeOptions, deps: Deps) {
  const sourceWindows: string[] = [];
  const redirects: RedirectRequest[] = [];
  const cancelled: number[] = [];
  const byRequest = new Map<string, OutgoingMail>();
  let nextOperation = 9000;

  const message = (id: MessageId) => deps.messages().find((m) => m.id === id);

  function source(id: MessageId): MessageSource | Promise<never> {
    const given = options.sources?.[id];
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    if (given) return isError(given) ? Promise.reject(given) : given;
    const item = message(id);
    if (!item) return deps.reject("not_found");
    const text = `From: ${item.from?.email ?? ""}\nSubject: ${item.subject}\n\nBody of ${item.subject}\n`;
    return { accountId: item.accountId, messageId: id, subject: item.subject, text, totalBytes: text.length, shownBytes: text.length, truncated: false };
  }

  function headers(id: MessageId): HeaderField[] | Promise<never> {
    const given = options.headers?.[id];
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    if (given) return isError(given) ? Promise.reject(given) : given;
    const item = message(id);
    if (!item) return deps.reject("not_found");
    return [
      { name: "From", value: item.from?.email ?? "" },
      { name: "Subject", value: item.subject },
    ];
  }

  function redirect(request: RedirectRequest, requestId: string): OutgoingMail | Promise<never> {
    const known = byRequest.get(requestId);
    if (known) return known;
    const item = message(request.messageId);
    if (!item) return deps.reject("not_found");
    const account = deps.accounts().find((a) => a.id === item.accountId);
    if (!account?.writesEnabled) return deps.reject("read_only");
    const all: Address[] = [...request.to, ...request.cc, ...request.bcc];
    if (all.length === 0) return deps.reject("invalid_address", "no recipient");
    const bad = all.find((a) => !/^[^\s@]+@[^\s@]+$/.test(a.email));
    if (bad) return deps.reject("invalid_address", bad.email);
    redirects.push(request);
    const now = Date.now();
    const delay = options.redirectDelaySeconds ?? 10;
    const mail: OutgoingMail = {
      operationId: nextOperation++,
      accountId: account.id,
      draftId: null,
      subject: item.subject,
      to: [...request.to, ...request.cc].slice(0, 5),
      recipientCount: all.length,
      state: delay > 0 ? "scheduled" : "queued",
      scheduled: false,
      sendAt: now + delay * 1000,
      error: null,
      attempts: 0,
      notice: null,
      archive: null,
      readReceipt: false,
      redirect: true,
      createdAt: now,
      updatedAt: now,
    };
    byRequest.set(requestId, mail);
    return mail;
  }

  return {
    state: {
      get sourceWindows() {
        return sourceWindows;
      },
      get redirects() {
        return redirects;
      },
      get cancelled() {
        return cancelled;
      },
    } satisfies Stage19FakeState,
    handle(cmd: string, args: Record<string, unknown>): unknown {
      switch (cmd) {
        case "open_source_window": {
          const accountId = args["accountId"] as AccountId;
          const messageId = args["messageId"] as MessageId;
          if (message(messageId)?.accountId !== accountId) return deps.reject("not_found");
          const key = `${accountId}/${String(messageId)}`;
          if (!sourceWindows.includes(key)) sourceWindows.push(key);
          return null;
        }
        case "get_message_source":
          return source(args["messageId"] as MessageId);
        case "get_message_headers":
          return headers(args["messageId"] as MessageId);
        case "redirect_message":
          return redirect(args["request"] as RedirectRequest, args["requestId"] as string);
        case "cancel_redirect": {
          const id = args["operationId"] as number;
          const mail = [...byRequest.values()].find((m) => m.operationId === id);
          if (!mail || cancelled.includes(id)) return deps.reject("not_found");
          cancelled.push(id);
          return null;
        }
        default:
          return undefined;
      }
    },
  };
}
