import { useState, type DragEvent } from "react";
import { useTranslation } from "react-i18next";
import type { Account, AccountId } from "../../services/ipc";
import { ACCOUNT_DRAG_TYPE, ACCOUNT_DROP_ZONE, isOwnDrag, useDragStore, type AccountPlace } from "../../stores/drag";
import { droppedOrder, useAccountArrange } from "./arrange";

/**
 * Stage 22 (decision 0078): an account heading in the left column (stage 23: also an account's dot
 * in the narrow rail) dragged to another place. A drag type of its own (`ACCOUNT_DRAG_TYPE`):
 * folders never take an account, and the lists of accounts never take dragged mail. The keyboard
 * and screen readers use "Move up" and "Move down" in the account's menu (or Alt+Up, Alt+Down on
 * the heading or the dot) instead.
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

/** An account's place on screen in a list of accounts, top to bottom. */
export interface AccountBox {
  id: AccountId;
  top: number;
  bottom: number;
}

/**
 * Stage 23: where a dragged account goes for a pointer at `y` over a list of accounts, as in
 * Thunderbird: before the first account whose middle lies below the pointer, else after the
 * last. Null when that leaves the order as it is (over the dragged account, or right next to it):
 * then no line shows and a drop does nothing.
 */
export function accountPlaceAt(
  y: number,
  boxes: readonly AccountBox[],
  accounts: readonly Account[],
  dragged: AccountId,
): { place: AccountPlace; order: AccountId[] } | null {
  const last = boxes.at(-1);
  if (!last) return null;
  const next = boxes.find((box) => y < (box.top + box.bottom) / 2);
  const place: AccountPlace = next ? { target: next.id, side: "before" } : { target: last.id, side: "after" };
  const order = droppedOrder(accounts, dragged, place.target, place.side === "after");
  return order ? { place, order } : null;
}

/** Marks one account of a list of accounts (`useAccountDropZone`). */
const ITEM = "data-account-drop-item";

function boxesIn(zone: HTMLElement): AccountBox[] {
  return [...zone.querySelectorAll<HTMLElement>(`[${ITEM}]`)].map((element) => {
    const box = element.getBoundingClientRect();
    return { id: element.getAttribute(ITEM) ?? "", top: box.top, bottom: box.bottom };
  });
}

/**
 * Stage 23: a list of accounts that takes a dragged account (the column's accounts, the rail's
 * dots). While the account is over the list, a line shows where it would go (`lineOf`); letting
 * go there puts it there (`onMoved` with the new order and the moved account's id). Where the order
 * would not change, nothing shows and the drop is refused; outside every list the line goes, and
 * a drop there does nothing. Escape cancels (the system ends its drag; the page forgets it too).
 */
export function useAccountDropZone(accounts: readonly Account[], onMoved: (order: AccountId[], moved: AccountId) => void) {
  const dragged = useDragStore((state) => state.account);
  const place = useDragStore((state) => state.accountPlace);
  const setPlace = useDragStore((state) => state.setAccountPlace);
  const endAccount = useDragStore((state) => state.endAccount);

  const resolve = (event: DragEvent<HTMLElement>) =>
    dragged && isOwnDrag(event, "account") ? accountPlaceAt(event.clientY, boxesIn(event.currentTarget), accounts, dragged) : null;

  const over = (event: DragEvent<HTMLElement>) => {
    if (!dragged || !isOwnDrag(event, "account")) return;
    const found = resolve(event);
    if (!found) {
      setPlace(null);
      return;
    }
    // Only where a drop changes something: the system shows the move cursor there.
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    setPlace(found.place);
  };

  return {
    /** The account being dragged right now (dimmed where it stands). */
    dragged,
    /** Spread on the element around the list. */
    zoneProps: {
      [ACCOUNT_DROP_ZONE]: "",
      onDragEnter: over,
      onDragOver: over,
      onDrop: (event: DragEvent<HTMLElement>) => {
        if (!dragged) return;
        const found = resolve(event);
        endAccount();
        if (!found) return;
        event.preventDefault();
        onMoved(found.order, dragged);
      },
    },
    /** Spread on each account's element in the list. */
    itemProps: (id: AccountId) => ({ [ITEM]: id }),
    /** Where the line shows at this account: in the gap before it, after it (the last), or none. */
    lineOf: (id: AccountId): AccountPlace["side"] | null => (dragged && place?.target === id ? place.side : null),
  };
}

/**
 * A list of accounts that takes a dragged account and puts it there for good (the core keeps the
 * order, stage 22), saying where it went (`announcement`, for a polite live region).
 */
export function useAccountMoveByDrag(accounts: readonly Account[]) {
  const { t } = useTranslation();
  const arrange = useAccountArrange();
  const [announcement, setAnnouncement] = useState("");
  const zone = useAccountDropZone(accounts, (order, movedId) => {
    const moved = accounts.find((account) => account.id === movedId);
    void arrange.reorder(order).then((ok) => {
      if (ok && moved) {
        setAnnouncement(t("accountEdit.moved", { name: moved.displayName || moved.email, position: order.indexOf(moved.id) + 1, count: order.length }));
      }
    });
  });
  return { ...zone, announcement };
}
