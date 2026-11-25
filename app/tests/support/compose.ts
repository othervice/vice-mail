// Stage 5: the fake core's drafts, sending, outbox view and compose settings, with the command
// names and argument shapes of the IPC contract (stage5-ipc-contract.md, decision 0018).
import type {
  Account,
  AccountSendSettings,
  Address,
  ComposeIdentity,
  ComposeSettings,
  ContactSuggestion,
  Draft,
  DraftAttachment,
  DraftSummary,
  DraftUpdate,
  IpcError,
  NewDraft,
  OutgoingMail,
  SentCopyMode,
  UnclearDecision,
  MissedSendDecision,
  SendLater,
} from "../../src/services/ipc";

import { MAX_SCHEDULE_AHEAD_MS } from "../../src/services/ipc";

function composeError(code: IpcError["code"], detail: string | null = null): IpcError {
  return { code, message: `English fallback for ${code}`, detail };
}

export function makeIdentity(overrides: Partial<ComposeIdentity> = {}): ComposeIdentity {
  return {
    id: 1,
    accountId: "11111111-1111-4111-8111-111111111111",
    email: "ada@vicemail.test",
    name: "Ada Lovelace",
    replyTo: null,
    isDefault: true,
    signature: null,
    signatureNew: null,
    signatureReply: null,
    ...overrides,
  };
}

let nextDraft = 1;

export function makeDraft(overrides: Partial<Draft> = {}): Draft {
  return {
    id: `draft-${String(nextDraft++)}`,
    accountId: "11111111-1111-4111-8111-111111111111",
    identityId: 1,
    kind: "new",
    to: [],
    cc: [],
    bcc: [],
    subject: "",
    format: "text",
    bodyHtml: "",
    bodyText: "",
    quoteHtml: null,
    quoteText: null,
    includeQuote: true,
    quotePosition: "below",
    includeSignature: true,
    signatureId: null,
    attachments: [],
    originalMessageId: null,
    revision: 0,
    state: "editing",
    server: "local_only",
    outgoing: null,
    archiveOriginal: false,
    requestReadReceipt: false,
    originalKept: false,
    createdAt: 1_700_000_000_000,
    updatedAt: 1_700_000_000_000,
    ...overrides,
  };
}

export function makeOutgoing(overrides: Partial<OutgoingMail> = {}): OutgoingMail {
  return {
    operationId: 500,
    accountId: "11111111-1111-4111-8111-111111111111",
    draftId: null,
    subject: "Hello",
    to: [{ name: "Grace Hopper", email: "grace@example.org" }],
    recipientCount: 1,
    state: "scheduled",
    scheduled: false,
    sendAt: Date.now() + 10_000,
    error: null,
    attempts: 0,
    notice: null,
    archive: null,
    readReceipt: false,
    redirect: false,
    createdAt: Date.now(),
    updatedAt: Date.now(),
    ...overrides,
  };
}

export interface ComposeFakeOptions {
  /** `listComposeIdentities` by account id. Default: one identity for every account. */
  identities?: Record<string, ComposeIdentity[]>;
  contacts?: ContactSuggestion[];
  settings?: ComposeSettings;
  /** Local drafts that exist already (restored after a crash, or frozen). */
  drafts?: Draft[];
  outgoing?: OutgoingMail[];
  /** What `createDraft` prepares for a reply, reply-all or forward, by message id. */
  replies?: Record<number, Partial<Draft>>;
  /** `openDraftFromMessage` by message id. */
  fromMessage?: Record<number, Partial<Draft>>;
  /** What the system dialog "picks" on `pickAttachments` (empty: the user cancelled). */
  picked?: DraftAttachment[];
  sentCopy?: SentCopyMode;
  serverSavesSent?: boolean;
  // ---- Stage 10: size check, inline images, pasting (the composer's block) ----
  /** What `checkDraftSize` answers (default: 2 kB, no limit known, nothing large). */
  sizeCheck?: Partial<import("../../src/services/ipc").DraftSizeCheck>;
  /** What the system dialog "picks" on `pickInlineImages` (empty: the user cancelled). */
  pickedImages?: DraftAttachment[];
  /** `cleanPastedHtml`; default: script, style, img and attributes other than href removed. */
  cleanPaste?: (html: string) => import("../../src/services/ipc").CleanedPaste;
}

export interface ComposeFakeState {
  /** Local drafts as the fake core holds them now. */
  readonly drafts: ReadonlyMap<string, Draft>;
  /** The outbox view, newest first. */
  readonly outgoing: readonly OutgoingMail[];
  readonly settings: ComposeSettings;
  /** Changes a send as the outbox would (the test then emits `outgoingChanged`). */
  setOutgoing: (operationId: number, patch: Partial<OutgoingMail>) => OutgoingMail;
  /** Ids the user discarded. */
  readonly deleted: readonly string[];
}

interface Deps {
  accounts: () => Account[];
}

const validEmail = (email: string) => /^[^\s@]+@[^\s@]+$/.test(email);

export function createComposeFake(options: ComposeFakeOptions, deps: Deps) {
  const drafts = new Map<string, Draft>((options.drafts ?? []).map((d) => [d.id, d]));
  let outgoing: OutgoingMail[] = [...(options.outgoing ?? [])];
  let settings: ComposeSettings = options.settings ?? { sendDelaySeconds: 10 };
  const identities = new Map<string, ComposeIdentity[]>(Object.entries(options.identities ?? {}));
  const sentCopy = new Map<string, SentCopyMode>();
  const requests = new Map<string, OutgoingMail>();
  const deleted: string[] = [];
  let nextOperation = 900;

  const identitiesOf = (accountId: string): ComposeIdentity[] => {
    const known = identities.get(accountId);
    if (known) return known;
    const account = deps.accounts().find((a) => a.id === accountId);
    const fallback = account
      ? [makeIdentity({ id: 1, accountId, email: account.email, name: account.displayName ?? "" })]
      : [];
    identities.set(accountId, fallback);
    return fallback;
  };

  const store = (draft: Draft): Draft => {
    drafts.set(draft.id, draft);
    return { ...draft };
  };

  const get = (draftId: unknown): Draft | undefined => drafts.get(draftId as string);

  function createDraft(request: NewDraft): Draft | Promise<never> {
    if (request.kind === "new") {
      const identity = identitiesOf(request.accountId).find((i) => i.isDefault) ?? identitiesOf(request.accountId)[0];
      return store(makeDraft({ accountId: request.accountId, identityId: request.identityId ?? identity?.id ?? null, to: request.to }));
    }
    if (request.kind === "compose") {
      // Stage 10: a mailto request with every field.
      const identity = identitiesOf(request.accountId).find((i) => i.isDefault) ?? identitiesOf(request.accountId)[0];
      const { to, cc, bcc, subject, body } = request.request;
      return store(
        makeDraft({ accountId: request.accountId, identityId: request.identityId ?? identity?.id ?? null, to, cc, bcc, subject: subject ?? "", bodyText: body ?? "" }),
      );
    }
    const prepared = options.replies?.[request.messageId];
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    if (!prepared) return Promise.reject(composeError("not_found"));
    const kind = request.kind === "forward_as_attachment" ? "forward" : request.kind === "edit_as_new" ? "new" : request.kind;
    // Stage 16: a new reply starts with the "send and archive" setting.
    const archiveOriginal = (kind === "reply" || kind === "reply_all") && Boolean(settings.archiveOnReply);
    return store(makeDraft({ kind, originalMessageId: request.messageId, archiveOriginal, ...prepared }));
  }

  function send(draftId: string, requestId: string): OutgoingMail | Promise<never> {
    const known = requests.get(requestId);
    if (known) return known;
    const draft = drafts.get(draftId);
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    if (!draft) return Promise.reject(composeError("not_found"));
    const account = deps.accounts().find((a) => a.id === draft.accountId);
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    if (!account?.writesEnabled) return Promise.reject(composeError("read_only"));
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    if (draft.state === "sending") return Promise.reject(composeError("send_in_progress"));
    const all = [...draft.to, ...draft.cc, ...draft.bcc];
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    if (all.length === 0) return Promise.reject(composeError("invalid_address", "no recipient"));
    const bad = all.find((a) => !validEmail(a.email));
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    if (bad) return Promise.reject(composeError("invalid_address", `not a valid address: ${bad.email}`));
    const operationId = nextOperation++;
    const now = Date.now();
    const delay = settings.sendDelaySeconds;
    const mail: OutgoingMail = {
      operationId,
      accountId: draft.accountId,
      draftId,
      subject: draft.subject,
      to: draft.to.slice(0, 5),
      recipientCount: all.length,
      state: delay > 0 ? "scheduled" : "queued",
      scheduled: false,
      sendAt: now + delay * 1000,
      error: null,
      attempts: 0,
      notice: null,
      archive: draft.archiveOriginal ? "waiting" : null,
      readReceipt: false,
      redirect: false,
      createdAt: now,
      updatedAt: now,
    };
    drafts.set(draftId, { ...draft, state: "sending", outgoing: operationId });
    outgoing = [mail, ...outgoing];
    requests.set(requestId, mail);
    return mail;
  }

  function cancelSend(draftId: string): Draft | Promise<never> {
    const draft = drafts.get(draftId);
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    if (!draft) return Promise.reject(composeError("not_found"));
    const mail = outgoing.find((m) => m.draftId === draftId && m.state !== "sent");
    if (mail && !["scheduled", "failed", "unclear", "missed"].includes(mail.state)) {
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
      return Promise.reject(composeError("send_in_progress"));
    }
    outgoing = outgoing.filter((m) => m !== mail);
    return store({ ...draft, state: "editing", outgoing: null });
  }

  // ---- Stage 10: send later ----
  function invalidSendAt(sendAt: number): Promise<never> | null {
    const now = Date.now();
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    if (sendAt <= now || sendAt - now > MAX_SCHEDULE_AHEAD_MS) return Promise.reject(composeError("invalid_argument", "send_at"));
    return null;
  }

  /** "Send now": the usual undo delay, no longer the user's time. */
  function sendNow(mail: OutgoingMail): OutgoingMail {
    const delay = settings.sendDelaySeconds;
    return { ...mail, state: delay > 0 ? "scheduled" : "queued", scheduled: false, sendAt: Date.now() + delay * 1000, error: null };
  }

  function replaceOutgoing(next: OutgoingMail): OutgoingMail {
    const changed = { ...next, updatedAt: Date.now() };
    outgoing = outgoing.map((m) => (m.operationId === changed.operationId ? changed : m));
    return changed;
  }

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "create_draft":
        return createDraft(args["request"] as NewDraft);
      case "get_draft": {
        const draft = get(args["draftId"]);
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return draft ? { ...draft } : Promise.reject(composeError("not_found"));
      }
      case "save_draft": {
        const draft = get(args["draftId"]);
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (!draft) return Promise.reject(composeError("not_found"));
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (draft.state === "sending") return Promise.reject(composeError("send_in_progress"));
        const update = args["update"] as DraftUpdate;
        const account = deps.accounts().find((a) => a.id === draft.accountId);
        return store({
          ...draft,
          ...update,
          revision: draft.revision + 1,
          server: account?.writesEnabled ? "pending" : "local_only",
          updatedAt: Date.now(),
        });
      }
      case "close_draft": {
        const draft = get(args["draftId"]);
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (!draft) return Promise.reject(composeError("not_found"));
        const account = deps.accounts().find((a) => a.id === draft.accountId);
        return store({ ...draft, server: account?.writesEnabled ? "saving" : "local_only" });
      }
      case "delete_draft": {
        const id = args["draftId"] as string;
        drafts.delete(id);
        deleted.push(id);
        return null;
      }
      case "list_drafts": {
        const accountId = args["accountId"] as string | null;
        return [...drafts.values()]
          .filter((d) => accountId === null || d.accountId === accountId)
          .sort((a, b) => b.updatedAt - a.updatedAt)
          .map(
            (d): DraftSummary => ({
              id: d.id,
              accountId: d.accountId,
              kind: d.kind,
              subject: d.subject,
              to: d.to.slice(0, 5),
              state: d.state,
              attachmentCount: d.attachments.length,
              updatedAt: d.updatedAt,
            }),
          );
      }
      case "open_draft_from_message": {
        const prepared = options.fromMessage?.[args["messageId"] as number];
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (!prepared) return Promise.reject(composeError("not_found"));
        return store(makeDraft(prepared));
      }
      case "pick_attachments": {
        const draft = get(args["draftId"]);
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (!draft) return Promise.reject(composeError("not_found"));
        const picked = (options.picked ?? []).filter((p) => !draft.attachments.some((a) => a.id === p.id));
        return store({ ...draft, attachments: [...draft.attachments, ...picked] });
      }
      case "add_dropped_attachments": {
        const draft = get(args["draftId"]);
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (!draft) return Promise.reject(composeError("not_found"));
        const paths = args["paths"] as string[];
        const added = paths.map((path, i) => ({
          id: `drop-${String(draft.attachments.length + i)}`,
          fileName: path.split("/").pop() ?? path,
          mime: "application/octet-stream",
          size: 1000,
          contentId: null,
        }));
        return store({ ...draft, attachments: [...draft.attachments, ...added] });
      }
      case "remove_draft_attachment": {
        const draft = get(args["draftId"]);
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (!draft) return Promise.reject(composeError("not_found"));
        return store({ ...draft, attachments: draft.attachments.filter((a) => a.id !== args["attachmentId"]) });
      }
      case "send_draft":
        return send(args["draftId"] as string, args["requestId"] as string);
      // Stage 16: "send and archive" and "request read receipt" of one draft.
      case "set_draft_send_options": {
        const draft = get(args["draftId"]);
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (!draft) return Promise.reject(composeError("not_found"));
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (draft.state === "sending") return Promise.reject(composeError("send_in_progress"));
        const options = args["options"] as { archiveOriginal: boolean; requestReadReceipt: boolean };
        return store({ ...draft, archiveOriginal: options.archiveOriginal, requestReadReceipt: options.requestReadReceipt });
      }
      case "cancel_send":
        return cancelSend(args["draftId"] as string);
      case "list_outgoing": {
        const before = args["before"] as number | null;
        return outgoing
          .filter((m) => m.accountId === args["accountId"] && (before === null || m.operationId < before))
          .slice(0, args["limit"] as number);
      }
      case "resolve_unclear_send": {
        const operationId = args["operationId"] as number;
        const decision = args["decision"] as UnclearDecision;
        const mail = outgoing.find((m) => m.operationId === operationId);
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (!mail) return Promise.reject(composeError("not_found"));
        const next: OutgoingMail = { ...mail, state: decision === "treat_as_sent" ? "sent" : "queued", error: null };
        outgoing = outgoing.map((m) => (m.operationId === operationId ? next : m));
        return next;
      }
      case "retry_operation": {
        // Only sends; everything else belongs to the stage 4 fake.
        const operationId = args["operationId"] as number;
        const mail = outgoing.find((m) => m.operationId === operationId);
        if (!mail) return undefined;
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (mail.state === "unclear") return Promise.reject(composeError("confirmation_required"));
        outgoing = outgoing.map((m) => (m.operationId === operationId ? { ...m, state: "queued", error: null } : m));
        return { batch: null, queued: 1, cancelled: 0 };
      }
      // ---- Stage 10: send later (local; the fake keeps the clock of the test) ----
      case "schedule_send": {
        const requestId = args["requestId"] as string;
        const known = requests.get(requestId);
        if (known) return known;
        const when = args["when"] as SendLater;
        const invalid = invalidSendAt(when.sendAt);
        if (invalid) return invalid;
        const started = send(args["draftId"] as string, requestId);
        if (started instanceof Promise) return started;
        const mail: OutgoingMail = { ...started, state: "scheduled", scheduled: true, sendAt: when.sendAt };
        outgoing = outgoing.map((m) => (m.operationId === mail.operationId ? mail : m));
        requests.set(requestId, mail);
        return mail;
      }
      case "list_scheduled_sends":
        return outgoing
          .filter((m) => (m.state === "scheduled" && m.scheduled) || m.state === "missed")
          .sort((a, b) => a.sendAt - b.sendAt);
      case "reschedule_send": {
        const operationId = args["operationId"] as number;
        const mail = outgoing.find((m) => m.operationId === operationId);
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (!mail) return Promise.reject(composeError("not_found"));
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (mail.state !== "scheduled" || !mail.scheduled) return Promise.reject(composeError("send_in_progress"));
        const when = args["when"] as SendLater | null;
        if (when) {
          const invalid = invalidSendAt(when.sendAt);
          if (invalid) return invalid;
        }
        return replaceOutgoing(when ? { ...mail, sendAt: when.sendAt, error: null } : sendNow(mail));
      }
      case "resolve_missed_send": {
        const operationId = args["operationId"] as number;
        const mail = outgoing.find((m) => m.operationId === operationId);
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (!mail) return Promise.reject(composeError("not_found"));
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (mail.state !== "missed") return Promise.reject(composeError("send_in_progress"));
        const decision = args["decision"] as MissedSendDecision;
        if (decision.kind === "send_now") return replaceOutgoing(sendNow(mail));
        if (decision.kind === "reschedule") {
          const invalid = invalidSendAt(decision.sendAt);
          if (invalid) return invalid;
          return replaceOutgoing({ ...mail, state: "scheduled", scheduled: true, sendAt: decision.sendAt, error: null });
        }
        outgoing = outgoing.filter((m) => m.operationId !== operationId);
        const draft = mail.draftId ? drafts.get(mail.draftId) : undefined;
        if (draft) drafts.set(draft.id, { ...draft, state: "editing", outgoing: null });
        return null;
      }
      case "get_compose_settings":
        return settings;
      case "set_compose_settings": {
        const next = args["settings"] as ComposeSettings;
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (next.sendDelaySeconds < 0 || next.sendDelaySeconds > 30) return Promise.reject(composeError("invalid_argument"));
        settings = next;
        return settings;
      }
      case "get_send_settings":
      case "set_sent_copy_mode": {
        const accountId = args["accountId"] as string;
        if (cmd === "set_sent_copy_mode") sentCopy.set(accountId, args["mode"] as SentCopyMode);
        const mode = sentCopy.get(accountId) ?? options.sentCopy ?? "auto";
        const result: AccountSendSettings = {
          accountId,
          sentCopy: mode,
          serverSavesSent: mode === "server" || (mode === "auto" && Boolean(options.serverSavesSent)),
          gmail: Boolean(options.serverSavesSent),
        };
        return result;
      }
      case "list_compose_identities":
        return identitiesOf(args["accountId"] as string).map((i) => ({ ...i }));
      case "set_identity_signature": {
        const identityId = args["identityId"] as number;
        const raw = args["signature"] as string | null;
        const signature = raw?.trim() ? raw : null;
        for (const [accountId, list] of identities) {
          const found = list.find((i) => i.id === identityId);
          if (found) {
            const next = { ...found, signature };
            identities.set(accountId, list.map((i) => (i.id === identityId ? next : i)));
            return next;
          }
        }
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return Promise.reject(composeError("not_found"));
      }
      case "search_contacts": {
        const words = (args["query"] as string).toLowerCase().split(/\s+/).filter(Boolean);
        return (options.contacts ?? [])
          .filter((c) => {
            const haystack = ` ${(c.name ?? "").toLowerCase()} ${c.email.toLowerCase().replace(/[@.]/g, " ")} ${c.email.toLowerCase()}`;
            return words.every((w) => haystack.includes(` ${w}`));
          })
          .slice(0, args["limit"] as number);
      }
      // ---- Stage 10: size check, inline images, pasting (the composer's block) ----
      case "check_draft_size": {
        const draft = get(args["draftId"]);
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (!draft) return Promise.reject(composeError("not_found"));
        const size = 2000 + Math.ceil((draft.attachments.reduce((sum, a) => sum + a.size, 0) * 4) / 3);
        const check = { size, serverLimit: null, limitKnown: false, exceedsServerLimit: false, large: false, ...options.sizeCheck };
        return check;
      }
      case "pick_inline_images":
      case "add_dropped_inline_images": {
        const draft = get(args["draftId"]);
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (!draft) return Promise.reject(composeError("not_found"));
        const added: DraftAttachment[] =
          cmd === "pick_inline_images"
            ? (options.pickedImages ?? []).filter((p) => !draft.attachments.some((a) => a.id === p.id))
            : (args["paths"] as string[]).map((path, i) => ({
                id: `img-drop-${String(draft.attachments.length + i)}`,
                fileName: path.split("/").pop() ?? path,
                mime: "image/png",
                size: 1000,
                contentId: `drop${String(draft.attachments.length + i)}@vicemail.test`,
              }));
        const next = store({ ...draft, attachments: [...draft.attachments, ...added] });
        return { draft: next, added };
      }
      case "inline_image_data": {
        const draft = get(args["draftId"]);
        const image = draft?.attachments.find((a) => a.id === args["attachmentId"] && a.contentId !== null);
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (!image) return Promise.reject(composeError("not_found"));
        return `data:${image.mime};base64,${btoa(image.id)}`;
      }
      case "clean_pasted_html": {
        const html = args["html"] as string;
        if (options.cleanPaste) return options.cleanPaste(html);
        const imagesRemoved = (html.match(/<img\b/gi) ?? []).length;
        const cleaned = html
          .replace(/<(script|style)\b[\s\S]*?<\/\1>/gi, "")
          .replace(/<img\b[^>]*>/gi, "")
          .replace(/\s(?!href=)[a-z-]+="[^"]*"/gi, "");
        return { html: cleaned, imagesRemoved, formattingSimplified: cleaned !== html };
      }
      default:
        return undefined;
    }
  }

  const state: ComposeFakeState = {
    get drafts() {
      return drafts;
    },
    get outgoing() {
      return outgoing;
    },
    get settings() {
      return settings;
    },
    setOutgoing: (operationId, patch) => {
      const mail = outgoing.find((m) => m.operationId === operationId);
      if (!mail) throw new Error(`no send ${String(operationId)}`);
      const next = { ...mail, ...patch, updatedAt: Date.now() };
      outgoing = outgoing.map((m) => (m.operationId === operationId ? next : m));
      return next;
    },
    get deleted() {
      return deleted;
    },
  };

  return { handle, state };
}

export type { Address };
