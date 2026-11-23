// Stage 9 and 13 gaps (decision 0027): the fake core's opened `.eml` files and `getMessagesByIds`,
// with the contract's rules (ids of 32 hex digits, at most MAX_MESSAGES_BY_IDS, unknown ids left out).
import {
  MAX_MESSAGES_BY_IDS,
  type EmlFile,
  type EmlFileId,
  type EmlMessageView,
  type IpcError,
  type MessageId,
  type MessageListItem,
  type OpenAttachmentResult,
  type SaveAttachmentResult,
} from "../../src/services/ipc";

export interface EmlFakeOptions {
  /** Opened files by id: their view, or the typed error the core refused the file with. */
  files?: Record<EmlFileId, EmlMessageView | IpcError>;
  /** `loadEmlRemoteContent` answers by file id. */
  remoteViews?: Record<EmlFileId, EmlMessageView>;
  /** The open dialog: a file opens (in a window of its own), the user cancels, or the core refuses it. */
  dialog?: { kind: "open"; file: EmlFile } | { kind: "cancel" } | { kind: "refuse"; error: IpcError };
}

export interface EmlFakeState {
  /** How often the open dialog was shown. */
  readonly dialogs: number;
  /** `saveEmlAttachment` and `openEmlAttachment` calls, as `<fileId>/<index>` (opened: with `!` when confirmed). */
  readonly saved: readonly string[];
  readonly opened: readonly string[];
  /** The id lists `getMessagesByIds` was asked for. */
  readonly byIds: readonly (readonly MessageId[])[];
}

interface Deps {
  /** Every message the fake core holds. */
  messages: () => MessageListItem[];
  reject: (code: IpcError["code"], detail?: string | null) => Promise<never>;
}

const FILE_ID = /^[0-9a-f]{32}$/;

function isError(value: EmlMessageView | IpcError): value is IpcError {
  return "code" in value;
}

export function createEmlFake(options: EmlFakeOptions, deps: Deps) {
  let dialogs = 0;
  const saved: string[] = [];
  const opened: string[] = [];
  const byIds: MessageId[][] = [];

  function file(args: Record<string, unknown>): EmlMessageView | Promise<never> {
    const fileId = args["fileId"] as string;
    if (!FILE_ID.test(fileId)) return deps.reject("invalid_argument", "the file id is not one the core made");
    const entry = options.files?.[fileId];
    if (!entry) return deps.reject("not_found");
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    if (isError(entry)) return Promise.reject(entry);
    return entry;
  }

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "open_eml_file": {
        dialogs += 1;
        const dialog = options.dialog ?? { kind: "cancel" };
        if (dialog.kind === "cancel") return { kind: "cancelled" };
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        if (dialog.kind === "refuse") return Promise.reject(dialog.error);
        return { kind: "opened", file: dialog.file };
      }
      case "get_eml_message":
        return file(args);
      case "load_eml_remote_content": {
        const view = file(args);
        if (view instanceof Promise) return view;
        return options.remoteViews?.[view.fileId] ?? view;
      }
      case "save_eml_attachment":
      case "open_eml_attachment": {
        const view = file(args);
        if (view instanceof Promise) return view;
        const index = args["index"] as number;
        const attachment = view.attachments.find((a) => a.index === index);
        if (!attachment) return deps.reject("not_found");
        if (cmd === "save_eml_attachment") {
          saved.push(`${view.fileId}/${String(index)}`);
          const result: SaveAttachmentResult = { kind: "saved", path: `/Users/ada/Downloads/${attachment.name}` };
          return result;
        }
        const confirmed = args["confirmedExecutable"] === true;
        if (attachment.executable && !confirmed) {
          const ask: OpenAttachmentResult = { kind: "needsConfirmation" };
          return ask;
        }
        opened.push(`${view.fileId}/${String(index)}${confirmed ? "!" : ""}`);
        const done: OpenAttachmentResult = { kind: "opened" };
        return done;
      }
      case "get_messages_by_ids": {
        const ids = args["messageIds"] as MessageId[];
        byIds.push([...ids]);
        if (ids.length > MAX_MESSAGES_BY_IDS) return deps.reject("invalid_argument", "too many ids");
        const all = deps.messages();
        const seen = new Set<MessageId>();
        const items: MessageListItem[] = [];
        for (const id of ids) {
          if (seen.has(id)) continue;
          seen.add(id);
          const item = all.find((m) => m.id === id);
          if (item) items.push(item);
        }
        return items;
      }
      default:
        return undefined;
    }
  }

  const state: EmlFakeState = {
    get dialogs() {
      return dialogs;
    },
    saved,
    opened,
    byIds,
  };

  return { handle, state };
}
