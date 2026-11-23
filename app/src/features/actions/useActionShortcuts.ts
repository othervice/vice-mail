import { useEffect, useEffectEvent } from "react";
import { useActionTarget } from "../../stores/actionTarget";
import { useNoticeStore } from "../../stores/notices";
import { boundShortcuts } from "../../stores/shortcuts";
import { ACTION_IDS, ACTIONS, actionState, type ActionContext, type ActionHost, type ActionId } from "./registry";
import { isEditableTarget, matchesShortcut, type ShortcutPlatform } from "./shortcuts";

interface ActionShortcutOptions {
  /** What the shortcuts act on; null: nothing (no shortcut is taken). */
  ctx: ActionContext | null;
  host: ActionHost;
  platform: ShortcutPlatform;
  /** Only these actions (a view without the pickers of the folder view offers fewer). */
  allowed: ReadonlySet<ActionId>;
  /** A picker or prompt is open: its keys win. */
  blocked: boolean;
  /** Whether a node lies in the view's list (shortcuts marked `listOnly`). */
  inList: (node: Node | null) => boolean;
}

/**
 * The keyboard shortcuts of the action directory for one view, the same way the folder view
 * handles them: not while typing in a field, and a read-only account gets the explanation
 * instead of nothing (stage 7: the unified inbox and search results use it).
 */
export function useActionShortcuts({ ctx, host, platform, allowed, blocked, inList }: ActionShortcutOptions): void {
  const push = useNoticeStore((state) => state.push);
  // Stage 8: the command palette runs the same actions on the same messages.
  useActionTarget({ ctx: blocked ? null : ctx, host, allowed });
  const onKey = useEffectEvent((event: KeyboardEvent) => {
    if (!ctx || blocked || event.defaultPrevented || event.repeat || isEditableTarget(event.target)) return;
    for (const id of ACTION_IDS) {
      if (!allowed.has(id)) continue;
      const definition = ACTIONS[id];
      // Stage 8: the user's assignment, read at the key press so a change works at once.
      if (!boundShortcuts(id, platform).some((shortcut) => matchesShortcut(event, shortcut, platform))) continue;
      if (definition.listOnly && !inList(event.target instanceof Node ? event.target : null)) continue;
      const state = actionState(definition, ctx);
      if (state.kind === "unavailable") continue;
      event.preventDefault();
      if (state.kind === "readOnly") push({ kind: "readOnly", accountId: ctx.account.id });
      else definition.run(ctx, host);
      return;
    }
  });
  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      onKey(event);
    };
    window.addEventListener("keydown", listener);
    return () => {
      window.removeEventListener("keydown", listener);
    };
  }, []);
}
