import { useEffect, useId, useRef, useState, type RefObject } from "react";
import { cx } from "../../components/cx";
import { useLayoutStore, type ReadingPanePosition } from "../../stores/layout";
import { registerListColumn } from "../../stores/toolbar";
import type { ListSelection, MessageListHandle } from "../messages/MessageList";

export interface ReadingLayout {
  position: ReadingPanePosition;
  /** Classes of the box that holds the list and the pane (and, between them, `ListResizeHandle`). */
  container: string;
  /** Id of the list column, which the handle between list and pane resizes (stage 18). */
  listId: string;
  /** Props of the list column, given whether something is selected. */
  listColumn: (open: boolean) => {
    id: string;
    className: string;
    inert?: boolean;
    "aria-hidden"?: true;
    /** Stage 23 (decision 0084): beside the pane, the toolbar's list segment ends where it ends. */
    ref?: (element: HTMLDivElement | null) => (() => void) | undefined;
  };
  /** Whether the pane (or the summary of a selection) is shown. */
  paneShown: (open: boolean) => boolean;
  /**
   * Stage 18 (task F): right of the list or below it, the pane's place stays while nothing is
   * selected and shows a quiet note (`NoMessageSelected`) instead of a list across the window.
   */
  placeholderShown: (open: boolean) => boolean;
  /** Stage 18 (decision 0054): the list selects its top message by itself (not in place of the list). */
  autoSelectFirst: boolean;
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
 * of the list. Stage 18: right of the list or below it, the border between them can be dragged
 * (`ListResizeHandle`); the list's size is a variable on the box that holds both, and the pane
 * keeps a minimum size, also when the window gets smaller. Task F: there the list keeps that size
 * with nothing selected too (the pane's place shows a quiet note), and opening a folder shows
 * its top message (decision 0054). In place of the list a message only shows once it is opened, because the list
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

  const listId = useId();
  // Decision 0031: the list column on its own slightly tinted surface, a hairline to the pane.
  const column = "flex flex-col bg-surface-list";
  return {
    position,
    container: cx("flex min-h-0 flex-1", position === "bottom" && "flex-col", replace && "relative"),
    listId,
    listColumn: (open) => {
      if (replace && opened && open) {
        return { id: listId, className: cx(column, "invisible absolute inset-0 min-h-0 min-w-0"), inert: true, "aria-hidden": true };
      }
      // Stage 18 (task F): beside or above the pane the list keeps its size, open or not.
      if (replace) return { id: listId, className: cx(column, "min-h-0 min-w-0 flex-1") };
      // Stage 18: the dragged size, never less than the list's minimum, never so much that the
      // pane gets less than its own (the list gives way first when the window gets smaller).
      return {
        id: listId,
        ...(position === "right" && { ref: registerListColumn }),
        className:
          position === "bottom"
            ? cx(
                column,
                "h-(--ui-list-height-reading) max-h-[calc(100%-var(--ui-reading-min-height))] min-h-(--ui-list-min-height-reading) min-w-0 shrink-0 border-b border-border-subtle",
              )
            : cx(
                column,
                "w-(--ui-list-width-reading) max-w-[calc(100%-var(--ui-reading-min-width))] min-w-(--ui-list-min-width-reading) min-h-0 shrink-0 border-e border-border-subtle",
              ),
      };
    },
    paneShown: (open) => (replace ? opened && open : open),
    placeholderShown: (open) => !replace && !open,
    autoSelectFirst: !replace,
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
