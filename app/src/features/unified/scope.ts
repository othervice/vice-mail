import type { MarkScope, UnifiedView } from "../../services/ipc";
import type { ShownMailViewId } from "../../stores/ui";

/**
 * Stage 23 (decision 0087): the views of "All accounts" in the column (and of the rail's first
 * group): besides the inbox, the views of marked messages, the pinned attachments and Sent. While
 * the unified scope is on (`unifiedInbox` in the UI store: every visible account, or a group's),
 * they cover its accounts in one mixed list; with an account chosen, that account as before.
 * Attachments, saved searches and contacts cover every account anyway; Drafts, Archive and Trash
 * stay folders of the chosen account (each account's section has them).
 */
const SCOPED_VIEWS: ReadonlySet<ShownMailViewId> = new Set([
  "inbox",
  "later",
  "waiting",
  "tasks",
  "readingList",
  "pinnedAttachments",
  "flagged",
  "pinned",
  "sent",
]);

export function coversScope(mailView: ShownMailViewId): boolean {
  return SCOPED_VIEWS.has(mailView);
}

/** The core's scope of marked messages for a unified scope. */
export function markScopeOf(view: UnifiedView): MarkScope {
  return view.kind === "all" ? { kind: "all" } : { kind: "group", groupId: view.groupId };
}
