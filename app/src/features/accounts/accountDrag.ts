import { useState, type DragEvent } from "react";
import type { Account } from "../../services/ipc";
import { ACCOUNT_DRAG_TYPE, useDragStore } from "../../stores/drag";
import { droppedOrder } from "./arrange";

/**
 * Stage 22 (decision 0078): an account heading in the left column dragged to another place. A drag
 * type of its own (`ACCOUNT_DRAG_TYPE`): folders never take it, and the account sections never take
 * dragged mail. The keyboard and screen readers use "Move up" and "Move down" in the account's menu
 * (or Alt+Up, Alt+Down on the heading) instead.
 */
export function useAccountDragSource(account: Account, enabled: boolean) {
  const startAccount = useDragStore((state) => state.startAccount);
  const endAccount = useDragStore((state) => state.endAccount);
  if (!enabled) return {};
  return {
    draggable: true,
    onDragStart: (event: DragEvent<HTMLElement>) => {
      event.stopPropagation();
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData(ACCOUNT_DRAG_TYPE, account.id);
      startAccount(account.id);
    },
    onDragEnd: () => {
      endAccount();
    },
  };
}

/**
 * An account's section in the left column takes a dragged account heading: before it in the upper
 * half, after it in the lower half, shown as a line in the gap (`position`). `onMoved` gets the new
 * order of the ids and the moved account's id.
 */
export function useAccountDropTarget(account: Account, accounts: readonly Account[], onMoved: (order: string[], moved: string) => void) {
  const dragged = useDragStore((state) => state.account);
  const endAccount = useDragStore((state) => state.endAccount);
  const [position, setPosition] = useState<"before" | "after" | null>(null);
  const carriesAccount = (event: DragEvent) => event.dataTransfer.types.includes(ACCOUNT_DRAG_TYPE);
  const after = (event: DragEvent<HTMLElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    return event.clientY > box.top + box.height / 2;
  };
  const over = (event: DragEvent<HTMLElement>) => {
    if (!dragged || !carriesAccount(event)) return;
    const next = after(event);
    if (!droppedOrder(accounts, dragged, account.id, next)) {
      if (position !== null) setPosition(null);
      return;
    }
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    const shown = next ? "after" : "before";
    if (position !== shown) setPosition(shown);
  };
  return {
    position: dragged ? position : null,
    handlers: {
      onDragEnter: over,
      onDragOver: over,
      onDragLeave: (event: DragEvent<HTMLElement>) => {
        // Only when the pointer leaves the section, not when it moves onto a row inside it.
        if (event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) return;
        setPosition(null);
      },
      onDrop: (event: DragEvent<HTMLElement>) => {
        setPosition(null);
        if (!dragged || !carriesAccount(event)) return;
        const order = droppedOrder(accounts, dragged, account.id, after(event));
        endAccount();
        if (!order) return;
        event.preventDefault();
        onMoved(order, dragged);
      },
    },
  };
}
