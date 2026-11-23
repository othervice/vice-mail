import type { AccountId, DraftId, EmlFileId, MessageId } from "../../services/ipc";

/**
 * Stage 8 (decision 0025): every window loads the same page; the query says what it shows.
 * `?window=message&accountId=<uuid>&messageId=<id>` is one message, `?window=composer&draftId=<uuid>`
 * the composer of one draft, no query the main window. Anything else is a broken window: it says
 * so instead of guessing.
 */
export type WindowRoute =
  | { kind: "main" }
  | { kind: "message"; accountId: AccountId; messageId: MessageId }
  | { kind: "composer"; draftId: DraftId }
  /** Stage 9 (decision 0027): `?window=eml&fileId=<32 hex digits>`, an opened `.eml` file. */
  | { kind: "eml"; fileId: EmlFileId }
  | { kind: "invalid" };

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function parseWindowRoute(search: string): WindowRoute {
  const params = new URLSearchParams(search);
  const kind = params.get("window");
  if (kind === null) return { kind: "main" };
  if (kind === "message") {
    const accountId = params.get("accountId") ?? "";
    const raw = params.get("messageId") ?? "";
    const messageId = /^[1-9][0-9]{0,15}$/.test(raw) ? Number(raw) : NaN;
    if (!UUID.test(accountId) || !Number.isSafeInteger(messageId)) return { kind: "invalid" };
    return { kind: "message", accountId: accountId.toLowerCase(), messageId };
  }
  if (kind === "composer") {
    const draftId = params.get("draftId") ?? "";
    if (!UUID.test(draftId)) return { kind: "invalid" };
    return { kind: "composer", draftId: draftId.toLowerCase() };
  }
  if (kind === "eml") {
    const fileId = params.get("fileId") ?? "";
    if (!/^[0-9a-f]{32}$/.test(fileId)) return { kind: "invalid" };
    return { kind: "eml", fileId };
  }
  return { kind: "invalid" };
}

/** The route of this window, read once (the query never changes while the page lives). */
export function currentWindowRoute(): WindowRoute {
  return parseWindowRoute(window.location.search);
}
