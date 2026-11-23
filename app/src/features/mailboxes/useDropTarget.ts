import { useState, type DragEvent } from "react";
import type { Mailbox } from "../../services/ipc";
import { acceptsDrop, MESSAGE_DRAG_TYPE, useDragStore } from "../../stores/drag";

/**
 * Makes a folder entry (sidebar tree, rail) a drop target for rows dragged out of the message
 * list: it accepts them only for another real folder of the same account, and says so while the
 * rows are over it (`over`), so the target is visible.
 */
export function useDropTarget(mailbox: Mailbox | undefined) {
  const session = useDragStore((state) => state.session);
  const end = useDragStore((state) => state.end);
  const [over, setOver] = useState(false);
  const accepts = acceptsDrop(session, mailbox);

  const carriesRows = (event: DragEvent) => event.dataTransfer.types.includes(MESSAGE_DRAG_TYPE);

  const handlers = {
    onDragEnter: (event: DragEvent) => {
      if (!accepts || !carriesRows(event)) return;
      event.preventDefault();
      setOver(true);
    },
    onDragOver: (event: DragEvent) => {
      if (!accepts || !carriesRows(event)) return;
      event.preventDefault();
      event.dataTransfer.dropEffect = "move";
      if (!over) setOver(true);
    },
    onDragLeave: () => {
      setOver(false);
    },
    onDrop: (event: DragEvent) => {
      setOver(false);
      if (!accepts || !session || !carriesRows(event)) return;
      event.preventDefault();
      // The rows leave the list right away, so their `dragend` may never reach it: end here.
      end();
      session.drop(mailbox);
    },
  };

  return { accepts, over: accepts && over, handlers };
}
