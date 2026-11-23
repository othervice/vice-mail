import { useEffect, useRef, useState, type RefObject } from "react";
import { cx } from "../../components/cx";
import { useLayoutStore, type ReadingPanePosition } from "../../stores/layout";
import type { ListSelection, MessageListHandle } from "../messages/MessageList";

export interface ReadingLayout {
  position: ReadingPanePosition;
  /** Classes of the box that holds the list and the pane. */
  container: string;
  /** Classes of the list column, given whether something is selected. */
  listColumn: (open: boolean) => string;
  /** The list column is hidden: a message shows in its place. */
  listHidden: (open: boolean) => boolean;
  /** Whether the pane (or the summary of a selection) is shown. */
  paneShown: (open: boolean) => boolean;
  /** The list asked to open a message (Enter, double click). */
  onOpen: () => void;
  onSelectionChange: (next: ListSelection) => void;
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
 * with its cursor where it was.
 */
export function useReadingLayout(list: RefObject<MessageListHandle | null>): ReadingLayout {
  const position = useLayoutStore((state) => state.readingPane);
  const [opened, setOpened] = useState(false);
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

  return {
    position,
    container: cx("flex min-h-0 flex-1", position === "bottom" && "flex-col"),
    listColumn: (open) => {
      if (!open || replace) return "flex min-h-0 min-w-0 flex-1 flex-col";
      return position === "bottom"
        ? "flex h-(--ui-list-height-reading) min-h-(--ui-list-min-height-reading) min-w-0 shrink-0 flex-col border-b border-border"
        : "flex min-h-0 min-w-0 w-(--ui-list-width-reading) max-w-(--ui-list-max-width-reading) shrink-0 flex-col border-e border-border";
    },
    listHidden: (open) => replace && opened && open,
    paneShown: (open) => (replace ? opened && open : open),
    onOpen: () => {
      if (replace) setOpened(true);
    },
    onSelectionChange: (next) => {
      // Several messages, or none: the list comes back (with the selection bar).
      if (replace && next.items.length !== 1) setOpened(false);
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
