import { useQueries } from "@tanstack/react-query";
import { useEffect, useRef, useState, type DragEvent } from "react";
import { ipc, type AccountId, type Mailbox } from "../../services/ipc";
import {
  dropModeOf,
  dropVerdict,
  isOwnDrag,
  useDragStore,
  type DragSession,
  type DropMode,
  type DropTarget,
  type DropVerdict,
} from "../../stores/drag";
import { useAccounts } from "../accounts/queries";
import { shortcutPlatform } from "../actions/shortcuts";
import { useAppInfo } from "../app-info/queries";
import { mailboxesQueryKey } from "./queries";

/** How long the rows rest on a closed folder before it opens (stage 22, decision 0081). */
export const EXPAND_DELAY_MS = 700;

interface DropTargetOptions {
  /** Tells this target apart from the others (the hint belongs to one target at a time). */
  id: string;
  /** The folder's name as shown, for the hint. */
  folder: string;
  /** A closed folder with subfolders: opens after the rows rested on it for a moment. */
  onExpand?: (() => void) | undefined;
}

/**
 * Makes a folder entry (column trees, favourites, rail) a drop target for rows dragged out of any
 * message list (stage 22, decision 0081). It takes them when the drop is possible: dropping moves,
 * Option (macOS) or Ctrl (Windows) copies, and `dropEffect` says which. While the rows are over
 * it, it says so (`over`), or why it does not take them (`refused`: a read-only account, a role
 * folder that is missing); the hint next to it shows and announces the same (`DropHint`). A
 * closed folder opens when the rows rest on it.
 */
export function useDropTarget(target: DropTarget | undefined, { id, folder, onExpand }: DropTargetOptions) {
  // `target` undefined: an entry that only opens while rows rest on it (a placeholder, an account).
  const session = useDragStore((state) => state.session);
  const end = useDragStore((state) => state.end);
  const setHover = useDragStore((state) => state.setHover);
  const clearHover = useDragStore((state) => state.clearHover);
  const accounts = useAccounts().data ?? [];
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  // The rail's roles need the folders of every account the rows come from.
  const roleAccounts: AccountId[] = target?.kind === "role" && session ? [...new Set(session.items.map((item) => item.accountId))] : [];
  const folders = useQueries({
    queries: roleAccounts.map((accountId) => ({ queryKey: mailboxesQueryKey(accountId), queryFn: () => ipc.listMailboxes(accountId) })),
  });
  const mailboxesOf = (accountId: AccountId): readonly Mailbox[] => folders[roleAccounts.indexOf(accountId)]?.data ?? [];
  const context = { accounts, mailboxesOf };
  // What the rows over this target were told, for the drag it belongs to (a new drag starts clean).
  const [hoveredBy, setHoveredBy] = useState<{ session: DragSession; verdict: DropVerdict } | null>(null);
  const hovered = hoveredBy && hoveredBy.session === session ? hoveredBy.verdict : null;
  const setHovered = (verdict: DropVerdict | null) => {
    setHoveredBy(verdict && session ? { session, verdict } : null);
  };
  const lastOver = useRef(0);
  const timer = useRef<number | null>(null);

  const verdictFor = (mode: DropMode) => dropVerdict(session, target, mode, context);
  const ready = verdictFor("move").kind === "accept" || verdictFor("copy").kind === "accept";
  // Stage 23: the store knows the rows; WebKit may hide the drag's own type until the drop.
  const carriesRows = (event: DragEvent) => isOwnDrag(event, "rows");

  const stopTimer = () => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = null;
  };
  useEffect(() => stopTimer, []);
  // The drag ended (dropped anywhere, or cancelled): no folder opens any more.
  useEffect(() => {
    if (!session) stopTimer();
  }, [session]);

  /** Starts the timer that opens a closed folder while the rows keep resting on it. */
  const armExpand = () => {
    if (!onExpand || timer.current !== null) return;
    timer.current = window.setTimeout(() => {
      timer.current = null;
      // Still over it (`dragover` keeps coming while the pointer rests there)?
      if (Date.now() - lastOver.current < 400) onExpand();
    }, EXPAND_DELAY_MS);
  };

  const over = (event: DragEvent) => {
    if (!session || !carriesRows(event)) return;
    lastOver.current = Date.now();
    // Also a placeholder or an account's row, which take no rows themselves, open.
    armExpand();
    if (!target) return;
    const verdict = verdictFor(dropModeOf(event, platform));
    if (verdict.kind === "none") return;
    if (verdict.kind === "accept") {
      event.preventDefault();
      event.dataTransfer.dropEffect = verdict.mode;
    } else {
      event.dataTransfer.dropEffect = "none";
    }
    const changed = hovered?.kind !== verdict.kind || (verdict.kind === "accept" && hovered.kind === "accept" && hovered.mode !== verdict.mode);
    if (changed) setHovered(verdict);
    const box = event.currentTarget.getBoundingClientRect();
    const current = useDragStore.getState().hover;
    // The same target, the same answer and still at its place (the column may have scrolled).
    const same =
      current?.id === id &&
      current.verdict.kind === verdict.kind &&
      (verdict.kind !== "accept" || (current.verdict.kind === "accept" && current.verdict.mode === verdict.mode)) &&
      Math.abs(current.rect.top - box.top) < 1;
    if (!same) setHover({ id, rect: { top: box.top, right: box.right, bottom: box.bottom, left: box.left }, verdict, folder });
  };

  const handlers = {
    onDragEnter: over,
    onDragOver: over,
    onDragLeave: () => {
      setHovered(null);
      clearHover(id);
    },
    onDrop: (event: DragEvent) => {
      stopTimer();
      setHovered(null);
      clearHover(id);
      if (!session || !target || !carriesRows(event)) return;
      const verdict = verdictFor(dropModeOf(event, platform));
      if (verdict.kind !== "accept") return;
      event.preventDefault();
      // The rows may leave the list right away, so their `dragend` may never reach it: end here.
      end();
      session.drop(target, verdict.mode);
    },
  };

  const shown = hovered;
  return {
    /** The running drag can be dropped here (as a move or a copy). */
    accepts: ready,
    over: shown?.kind === "accept",
    /** Over it, and it does not take the rows (its hint says why). */
    refused: shown?.kind === "refuse",
    /** Over it as a copy. */
    copying: shown?.kind === "accept" && shown.mode === "copy",
    handlers,
  };
}

/** `data-drop-target` of a drop target: what the desktop tests and the styles look at. */
export function dropTargetState(drop: ReturnType<typeof useDropTarget>): "over" | "refused" | "ready" | undefined {
  if (drop.over) return "over";
  if (drop.refused) return "refused";
  return drop.accepts ? "ready" : undefined;
}

/**
 * A target that does not take the rows while they are over it: a dashed outline in the danger
 * colour (with the cursor's "no" and the hint beside it, never the colour alone).
 */
export const DROP_REFUSED_CLASS = "outline-2 -outline-offset-2 outline-dashed outline-danger";
