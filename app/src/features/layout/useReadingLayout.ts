import { useEffect, useRef, useState, type RefObject } from "react";
import { cx } from "../../components/cx";
import { useLayoutStore, type ReadingPanePosition } from "../../stores/layout";
import type { ListSelection, MessageListHandle } from "../messages/MessageList";

export interface ReadingLayout {
  position: ReadingPanePosition;
  /** Classes of the box that holds the list and the pane. */
  container: string;
  /** Props of the list column, given whether something is selected. */
  listColumn: (open: boolean) => { className: string; inert?: boolean; "aria-hidden"?: true };
  /** Whether the pane (or the summary of a selection) is shown. */
  paneShown: (open: boolean) => boolean;
  /** The list asked to open a message (Enter, double click). */
  onOpen: () => void;
  /**
   * The list's selection changed. True when the pane should take the focus for the new message:
   * in place of the list, an action moved on to the next message while the list is hidden.
   */
  onSelectionChange: (next: ListSelection) => boolean;
  /** Escape in the pane: back to the list (in place of the list: the pane closes first). */
  backToList: (focusList: () => void) => void;
  /** The pane's close button: in place of the list back to it, otherwise `clearSelection`. */
  close: (clearSelection: () => void) => void;
}

/**
 * Where the reading pane sits (stage 8, docs/DESIGN.md "Lesebereich"): right of the list (the
 * list gets narrower and its rows two lines), below it (the list keeps its width), or in place
 * of the list. In place of the list a message only shows once it is opened, because the list
 * selects a message as soon as it has the focus; Escape or the close button bring the list back
 * with its cursor and scroll position where they were. Meanwhile the list stays laid out but
 * invisible and inert (not `display: none`, which would lose the scroll position), and after an
 * action the next message takes its place with the focus.
 */
export function useReadingLayout(list: RefObject<MessageListHandle | null>): ReadingLayout {
  const position = useLayoutStore((state) => state.readingPane);
  const [opened, setOpened] = useState(false);
  // A new position starts closed (an earlier "opened" must not show up again later).
  const [openedFor, setOpenedFor] = useState(position);
  if (openedFor !== position) {
    setOpenedFor(position);
    setOpened(false);
  }
  const refocusList = useRef(false);
  const replace = position === "replace";

  // The list is hidden while a message shows in its place; it can take the focus once it is back.
  useEffect(() => {
    if (!refocusList.current || (replace && opened)) return;
    refocusList.current = false;
    list.current?.focus();
  });

  const back = () => {
    refocusList.current = true;
    setOpened(false);
  };

  // Decision 0031: the list column on its own slightly tinted surface, a hairline to the pane.
  const column = "flex min-h-0 min-w-0 flex-col bg-surface-list";
  return {
    position,
    container: cx("flex min-h-0 flex-1", position === "bottom" && "flex-col", replace && "relative"),
    listColumn: (open) => {
      if (replace && opened && open) {
        return { className: cx(column, "invisible absolute inset-0"), inert: true, "aria-hidden": true };
      }
      if (!open || replace) return { className: cx(column, "flex-1") };
      return {
        className:
          position === "bottom"
            ? cx(column, "h-(--ui-list-height-reading) min-h-(--ui-list-min-height-reading) shrink-0 border-b border-border-subtle")
            : cx(column, "w-(--ui-list-width-reading) max-w-(--ui-list-max-width-reading) shrink-0 border-e border-border-subtle"),
      };
    },
    paneShown: (open) => (replace ? opened && open : open),
    onOpen: () => {
      if (replace) setOpened(true);
    },
    onSelectionChange: (next) => {
      if (!replace || !opened) return false;
      // Several messages, or none (the last one was archived): the list comes back.
      if (next.items.length !== 1) {
        back();
        return false;
      }
      return true;
    },
    backToList: (focusList) => {
      if (replace) back();
      else focusList();
    },
    close: (clearSelection) => {
      if (replace) back();
      else clearSelection();
    },
  };
}
