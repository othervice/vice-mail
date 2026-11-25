// Stage 19 (decision 0055): the fake core's source window, all headers and redirecting, with the
// contract's rules (one window per message, `read_only` without writes, the same request id once,
// `cancelRedirect` only while it waits).
import type {
  Account,
  AccountId,
  Address,
  ContactList,
  ContactListSummary,
  HeaderField,
  IpcError,
  MailboxId,
  MboxJob,
  MessageId,
  MessageListItem,
  MessageSource,
  OutgoingMail,
  RedirectRequest,
  TakeoverPreview,
  ThunderbirdProfile,
  VcardExportOutcome,
  VcardImportOutcome,
} from "../../src/services/ipc";

export interface Stage19FakeOptions {
  /** `getMessageSource` answers by message id; default: a small text made from the list row. */
  sources?: Record<MessageId, MessageSource | IpcError>;
  /** `getMessageHeaders` answers by message id; default: From, To, Subject of the row. */
  headers?: Record<MessageId, HeaderField[] | IpcError>;
  /** The undo delay of redirects in seconds (default 10). */
  redirectDelaySeconds?: number;
  /** Contact groups the fake starts with. */
  lists?: ContactList[];
  /** What "Import vCard…" answers (default: cancelled). */
  vcardImport?: VcardImportOutcome;
  /** What an export answers (default: saved to ~/Downloads/contacts.vcf). */
  vcardExport?: VcardExportOutcome;
  /** Thunderbird profiles at the usual place, and the preview each opens (by index). */
  profiles?: ThunderbirdProfile[];
  previews?: TakeoverPreview[];
}

export interface Stage19FakeState {
  /** Source windows opened, as `<accountId>/<messageId>`, once each. */
  readonly sourceWindows: readonly string[];
  readonly redirects: readonly RedirectRequest[];
  readonly cancelled: readonly number[];
  /** The groups as the fake holds them now. */
  readonly lists: readonly ContactList[];
  /** mbox jobs started, and cancel requests. */
  readonly mboxJobs: readonly MboxJob[];
  readonly mboxCancelled: readonly string[];
  /** Identities and signatures taken over: `<token>/<index>/<accountId>`. */
  readonly applied: readonly string[];
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
  let lists: ContactList[] = [...(options.lists ?? [])];
  const mboxJobs: MboxJob[] = [];
  const mboxCancelled: string[] = [];
  const applied: string[] = [];
  let nextList = 1;
  let nextJob = 1;
  const summary = (l: ContactList): ContactListSummary => ({ id: l.id, name: l.name, memberCount: l.members.length });
  const list = (id: string) => lists.find((l) => l.id === id);
  const job = (kind: MboxJob["kind"], mailboxId: MailboxId, fileName: string): MboxJob => {
    const accountId = deps.messages()[0]?.accountId ?? deps.accounts()[0]?.id ?? "";
    const started: MboxJob = {
      id: (nextJob++).toString(16).padStart(32, "0"),
      kind,
      accountId,
      mailboxId,
      mailboxName: "INBOX",
      fileName,
      state: "running",
      total: kind === "export" ? 3 : null,
      done: 0,
      queued: 0,
      skipped: 0,
      error: null,
    };
    mboxJobs.push(started);
    return started;
  };

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
      get lists() {
        return lists;
      },
      get mboxJobs() {
        return mboxJobs;
      },
      get mboxCancelled() {
        return mboxCancelled;
      },
      get applied() {
        return applied;
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
        case "list_contact_lists":
          return [...lists].sort((a, b) => a.name.localeCompare(b.name)).map(summary);
        case "get_contact_list":
          return list(args["id"] as string) ?? deps.reject("not_found");
        case "create_contact_list": {
          const name = (args["name"] as string).trim();
          if (!name) return deps.reject("invalid_argument", "name");
          const created: ContactList = {
            id: `0b5e8f43-6c1f-4a51-9a51-${nextList.toString(16).padStart(12, "0")}`,
            name,
            members: (args["members"] as Address[]).map((a) => ({ email: a.email, name: a.name, contact: null })),
          };
          nextList += 1;
          lists.push(created);
          return created;
        }
        case "rename_contact_list": {
          const found = list(args["id"] as string);
          if (!found) return deps.reject("not_found");
          found.name = (args["name"] as string).trim();
          return found;
        }
        case "delete_contact_list":
          lists = lists.filter((l) => l.id !== args["id"]);
          return null;
        case "add_contact_list_members": {
          const found = list(args["id"] as string);
          if (!found) return deps.reject("not_found");
          for (const a of args["members"] as Address[]) {
            if (!found.members.some((m) => m.email.toLowerCase() === a.email.toLowerCase()))
              found.members.push({ email: a.email, name: a.name, contact: null });
          }
          return found;
        }
        case "remove_contact_list_member": {
          const found = list(args["id"] as string);
          if (!found) return deps.reject("not_found");
          const email = (args["email"] as string).toLowerCase();
          found.members = found.members.filter((m) => m.email.toLowerCase() !== email);
          return found;
        }
        case "suggest_contact_lists": {
          const words = (args["query"] as string).toLowerCase().split(/\s+/).filter(Boolean);
          return lists
            .filter((l) => words.every((w) => l.name.toLowerCase().split(/\s+/).some((n) => n.startsWith(w))))
            .map(summary);
        }
        case "import_vcards":
          return options.vcardImport ?? { kind: "cancelled" };
        case "export_vcards":
          return options.vcardExport ?? { kind: "saved", path: "/Users/test/Downloads/contacts.vcf", cards: 2 };
        case "export_mailbox_mbox":
          return { kind: "started", job: job("export", args["mailboxId"] as MailboxId, "INBOX.mbox") };
        case "import_mailbox_mbox": {
          const mailboxId = args["mailboxId"] as MailboxId;
          const accountId = deps.messages().find((m) => m.mailboxId === mailboxId)?.accountId ?? deps.accounts()[0]?.id;
          const account = deps.accounts().find((a) => a.id === accountId);
          if (!account?.writesEnabled) return deps.reject("read_only");
          return { kind: "started", job: job("import", mailboxId, "Inbox") };
        }
        case "cancel_mbox_job": {
          const id = args["id"] as string;
          const found = mboxJobs.find((j) => j.id === id);
          if (!found) return deps.reject("not_found");
          mboxCancelled.push(id);
          return found;
        }
        case "list_mbox_jobs":
          return mboxJobs;
        case "list_thunderbird_profiles":
          return options.profiles ?? [];
        case "open_thunderbird_profile":
          return options.previews?.[args["index"] as number] ?? deps.reject("not_found");
        case "choose_thunderbird_profile":
          return options.previews?.[0] ? { kind: "opened", preview: options.previews[0] } : { kind: "cancelled" };
        case "apply_thunderbird_account":
          applied.push(`${String(args["token"])}/${String(args["index"])}/${String(args["accountId"])}`);
          return { accountId: args["accountId"], identities: 1, signatures: 2 };
        case "import_thunderbird_folder":
          return job("import", args["mailboxId"] as MailboxId, "Inbox");
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
