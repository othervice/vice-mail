import { useState, type FocusEvent, type KeyboardEvent, type PointerEvent } from "react";
import type { ListSelection } from "../messages/MessageList";

export interface Previewing {
  /**
   * The pane shows the top message the list chose by itself (decision 0054) and the user has not
   * worked with it yet: "Mark as read when opened" waits.
   */
  previewing: boolean;
  /** The user works with the message (opened it from the list, Enter or a double click). */
  engage: () => void;
  /** For a wrapper around the pane: a press, a key or the focus inside it engage. */
  paneProps: {
    onPointerDownCapture: (event: PointerEvent) => void;
    onKeyDownCapture: (event: KeyboardEvent) => void;
    onFocusCapture: (event: FocusEvent) => void;
  };
}

/**
 * Stage 18 (task F, decision 0054): a message the list opened by itself when the folder opened is
 * not read yet. It counts as opened by the user once the user selects a row (the list then
 * reports no `auto`), opens it from the list, or presses, types or moves the focus into the pane.
 */
export function usePreviewing(selection: ListSelection): Previewing {
  const key = selection.keys[0] ?? "";
  const [engaged, setEngaged] = useState<string | null>(null);
  const engage = () => {
    if (key !== "" && engaged !== key) setEngaged(key);
  };
  return {
    previewing: selection.auto === true && engaged !== key,
    engage,
    paneProps: { onPointerDownCapture: engage, onKeyDownCapture: engage, onFocusCapture: engage },
  };
}
