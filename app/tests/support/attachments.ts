// Stage 16 (decision 0036) part of the fake core: attachment previews, "Save all", pinned
// attachments and the count of mail that blocked senders leave out of an inbox list, with the
// command names and argument shapes of the IPC contract.
import type {
  AttachmentEntry,
  AttachmentFilter,
  AttachmentInfo,
  MailboxQuota,
  AttachmentPreview,
  InboxScope,
  IpcError,
  MarkScope,
  MessageListItem,
  PinnedAttachment,
  SaveAllAttachmentsResult,
} from "../../src/services/ipc";

/** A preview answer, or the typed error the core refuses it with. */
export type PreviewAnswer = AttachmentPreview | IpcError | ((page: number) => AttachmentPreview | IpcError);

export interface AttachmentsFakeOptions {
  /** `previewAttachment` answers by `<messageId>/<index>` (or `<fileId>/<index>` for `.eml` files). */
  previews?: Record<string, PreviewAnswer>;
  /** `saveAllAttachments` answers by message id (or file id); default: saved to ~/Downloads. */
  saveAll?: Record<string, SaveAllAttachmentsResult | IpcError>;
  /** Pinned attachments as the core holds them, the newest pin first. */
  pinned?: PinnedAttachment[];
  /** `countBlockedHidden` by scope (`JSON.stringify(scope)`); default 0. */
  blockedHidden?: Record<string, number>;
  /** Stage 20: the attachments view's entries, newest first (filtered here like the core). */
  attachmentEntries?: AttachmentEntry[];
  /** Stage 20: `getAccountQuota` by account id; default: the server reports none. */
  quotas?: Record<string, MailboxQuota>;
}

export interface AttachmentsFakeState {
  readonly pinned: readonly PinnedAttachment[];
  /** Pages asked for, as `<owner>/<index>/<page>`. */
  readonly previews: readonly string[];
  /** `saveAll…` calls by owner id. */
  readonly savedAll: readonly string[];
  setBlockedHidden: (scope: InboxScope, count: number) => void;
}

interface Deps {
  messages: () => MessageListItem[];
  /** The attachments of a stored message (from its view), to pin them. */
  attachmentsOf: (messageId: number) => readonly AttachmentInfo[];
  reject: (code: IpcError["code"], detail?: string | null) => Promise<never>;
}

const isError = (value: unknown): value is IpcError =>
  typeof value === "object" && value !== null && "code" in value && "message" in value;

function inScope(item: MessageListItem, scope: MarkScope): boolean {
  return scope.kind !== "account" || item.accountId === scope.accountId;
}

export function createAttachmentsFake(options: AttachmentsFakeOptions, deps: Deps) {
  let pinned = [...(options.pinned ?? [])];
  const previews: string[] = [];
  const savedAll: string[] = [];
  const hidden = new Map(Object.entries(options.blockedHidden ?? {}));
  let clock = 1_800_000_000_000;

  function preview(owner: string, index: number, page: number): unknown {
    previews.push(`${owner}/${String(index)}/${String(page)}`);
    const answer = options.previews?.[`${owner}/${String(index)}`];
    if (answer === undefined) return deps.reject("preview_unsupported");
    const value = typeof answer === "function" ? answer(page) : answer;
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    if (isError(value)) return Promise.reject(value);
    return value;
  }

  function saveAll(owner: string): unknown {
    savedAll.push(owner);
    const answer = options.saveAll?.[owner] ?? { kind: "saved", folder: "/Users/ada/Downloads", saved: 2, renamed: 0 };
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    if (isError(answer)) return Promise.reject(answer);
    return answer;
  }

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "preview_attachment":
        return preview(String(args["messageId"]), args["index"] as number, args["page"] as number);
      case "preview_eml_attachment":
        return preview(String(args["fileId"]), args["index"] as number, args["page"] as number);
      case "save_all_attachments":
        return saveAll(String(args["messageId"]));
      case "save_all_eml_attachments":
        return saveAll(String(args["fileId"]));
      case "set_attachment_pinned": {
        const messageId = args["messageId"] as number;
        const index = args["index"] as number;
        const message = deps.messages().find((m) => m.id === messageId);
        if (!message) return deps.reject("not_found");
        pinned = pinned.filter((entry) => !(entry.message.id === messageId && entry.index === index));
        if (args["pinned"] === true) {
          const attachment = deps.attachmentsOf(messageId).find((a) => a.index === index);
          if (!attachment) return deps.reject("not_found");
          clock += 1000;
          pinned.unshift({
            message,
            index,
            name: attachment.name,
            mimeType: attachment.mimeType,
            size: attachment.size,
            preview: attachment.preview,
            pinnedAt: clock,
          });
        }
        return null;
      }
      case "get_attachment_pins": {
        const messageId = args["messageId"] as number;
        return pinned
          .filter((entry) => entry.message.id === messageId)
          .map((entry) => entry.index)
          .sort((a, b) => a - b);
      }
      case "list_pinned_attachments": {
        const scope = args["scope"] as MarkScope;
        return pinned.filter((entry) => inScope(entry.message, scope));
      }
      case "count_blocked_hidden":
        return hidden.get(JSON.stringify(args["scope"])) ?? 0;
      // Stage 20 (decision 0060).
      case "list_attachments": {
        const filter = args["filter"] as AttachmentFilter;
        const limit = args["limit"] as number;
        const name = filter.query?.trim().toLowerCase() ?? "";
        const sender = filter.sender?.trim().toLowerCase() ?? "";
        const items = (options.attachmentEntries ?? []).filter(
          (entry) =>
            (filter.scope.kind !== "account" || entry.accountId === filter.scope.accountId) &&
            (filter.kind === null || entry.kind === filter.kind) &&
            entry.name.toLowerCase().includes(name) &&
            (sender === "" ||
              (entry.from?.email.toLowerCase().includes(sender) ?? false) ||
              (entry.from?.name?.toLowerCase().includes(sender) ?? false)),
        );
        return { items: items.slice(0, limit), nextCursor: null };
      }
      case "get_account_quota":
        return options.quotas?.[args["accountId"] as string] ?? { kind: "notReported" };
      default:
        return undefined;
    }
  }

  const state: AttachmentsFakeState = {
    get pinned() {
      return pinned;
    },
    previews,
    savedAll,
    setBlockedHidden: (scope, count) => {
      hidden.set(JSON.stringify(scope), count);
    },
  };
  return { handle, state };
}
