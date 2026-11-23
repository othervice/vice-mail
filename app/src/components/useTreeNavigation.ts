import { useCallback, useLayoutEffect, useRef, useState, type FocusEvent, type KeyboardEvent, type MouseEvent } from "react";

/** One visible row of a tree, in the order shown (children right after their parent). */
export interface TreeNavItem {
  /** Stable and unique within the tree. */
  key: string;
  /** Depth, from 1 for the top level (`aria-level`). */
  level: number;
  /** Place among its siblings, from 1, and their number (`aria-posinset`, `aria-setsize`). */
  posinset: number;
  setsize: number;
  /** Open or closed for a row with children; `undefined` for a leaf. */
  expanded?: boolean | undefined;
  /** The visible name, for type-ahead. */
  text: string;
}

interface TreeNavigationOptions {
  items: readonly TreeNavItem[];
  /** The row of the view shown right now (`aria-current`): the tab stop when Tab comes in. */
  currentKey: string | null;
  /** Enter, Space (and a click): opens what the row stands for. */
  onOpen: (key: string) => void;
  /** Right and Left on a row with children: opens or closes it. */
  onExpand?: (key: string, expanded: boolean) => void;
  /** Shift+F10, the context-menu key or a right click: the row's menu (default: its "…" button). */
  onMenu?: (key: string) => void;
}

/** Letters and digits: type-ahead. Punctuation ("/" search, "?" shortcuts) stays a shortcut. */
const TYPEAHEAD = /^[\p{L}\p{N}]$/u;
/** Keys typed within this time form one search ("pr" finds "Projekte"). */
const TYPEAHEAD_MS = 500;

function rowIn(tree: HTMLElement | null, key: string): HTMLElement | undefined {
  return [...(tree?.querySelectorAll<HTMLElement>("[data-tree-key]") ?? [])].find((row) => row.dataset["treeKey"] === key);
}

function startsWith(text: string, query: string): boolean {
  return text.slice(0, query.length).localeCompare(query, undefined, { sensitivity: "base" }) === 0;
}

/** Aria attributes of a row: level, position among its siblings, open or closed. */
export function treeItemAria(item: TreeNavItem) {
  return {
    role: "treeitem" as const,
    "aria-level": item.level,
    "aria-posinset": item.posinset,
    "aria-setsize": item.setsize,
    "aria-expanded": item.expanded,
  };
}

/**
 * Keyboard model of a sidebar tree (WAI-ARIA tree view, navigation variant): the tree is a single
 * tab stop with a roving tabindex. Tab comes in on the current row (else the first), Up and Down
 * move, Right opens a closed row or goes to its first child, Left closes an open row or goes to
 * its parent, Home and End go to the first and last row, a letter or digit goes to the next row
 * whose name starts with it, Enter and Space open the row, Shift+F10 or the context-menu key opens
 * its menu. Moving does not open anything: the list changes only on Enter, Space or a click.
 *
 * Rows are flat in the DOM (the hierarchy is in `aria-level`, `aria-posinset`, `aria-setsize`), so
 * the focus ring frames one row and never the open subtree below it.
 */
export function useTreeNavigation({ items, currentKey, onOpen, onExpand, onMenu }: TreeNavigationOptions) {
  // The row with the focus while it is inside the tree; null once it left.
  const [focusKey, setFocusKey] = useState<string | null>(null);
  const tree = useRef<HTMLElement | null>(null);
  const typed = useRef({ text: "", at: Number.NEGATIVE_INFINITY });
  // A row that should keep the focus while it moves (a reordered favourite).
  const keep = useRef<string | null>(null);

  const has = (key: string | null) => key !== null && items.some((item) => item.key === key);
  const tabStop = has(focusKey) ? focusKey : has(currentKey) ? currentKey : (items[0]?.key ?? null);

  const focusRow = (key: string | undefined) => {
    if (key === undefined) return;
    setFocusKey(key);
    rowIn(tree.current, key)?.focus();
  };

  /** The row's menu: `onMenu`, else the row's own menu button ("…"), opened once. */
  const openMenu = (key: string) => {
    if (onMenu) {
      onMenu(key);
      return;
    }
    const button = rowIn(tree.current, key)?.querySelector<HTMLElement>('[aria-haspopup="menu"]');
    if (button && button.getAttribute("aria-expanded") !== "true") button.click();
  };

  const order = items.map((item) => item.key).join("\n");
  useLayoutEffect(() => {
    const key = keep.current;
    if (key === null) return;
    keep.current = null;
    const row = rowIn(tree.current, key);
    const now = document.activeElement;
    // Moving the row in the DOM drops its focus; never take it from somewhere else, though.
    if (row && now !== row && (now === null || now === document.body || tree.current?.contains(now))) row.focus();
  }, [order]);

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    // Only keys on a row itself: a menu or button inside a row handles its own.
    const target = event.target instanceof HTMLElement ? event.target : null;
    const key = target?.dataset["treeKey"];
    if (event.defaultPrevented || key === undefined) return;
    const index = items.findIndex((item) => item.key === key);
    const item = items[index];
    if (!item) return;
    if ((event.shiftKey && event.key === "F10") || event.key === "ContextMenu") {
      event.preventDefault();
      openMenu(key);
      return;
    }
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    const handled = () => {
      event.preventDefault();
      event.stopPropagation();
    };
    switch (event.key) {
      case "ArrowDown":
        handled();
        focusRow(items[index + 1]?.key);
        return;
      case "ArrowUp":
        handled();
        focusRow(items[index - 1]?.key);
        return;
      case "Home":
        handled();
        focusRow(items[0]?.key);
        return;
      case "End":
        handled();
        focusRow(items.at(-1)?.key);
        return;
      case "ArrowRight":
        handled();
        if (item.expanded === false) onExpand?.(key, true);
        else if (item.expanded === true && (items[index + 1]?.level ?? 0) > item.level) focusRow(items[index + 1]?.key);
        return;
      case "ArrowLeft": {
        handled();
        if (item.expanded === true) {
          onExpand?.(key, false);
          return;
        }
        for (let before = index - 1; before >= 0; before -= 1) {
          const parent = items[before];
          if (parent && parent.level < item.level) {
            focusRow(parent.key);
            return;
          }
        }
        return;
      }
      case "Enter":
      case " ":
        handled();
        if (!event.repeat) onOpen(key);
        return;
      default:
    }
    if (!TYPEAHEAD.test(event.key)) return;
    handled();
    const recent = event.timeStamp - typed.current.at < TYPEAHEAD_MS;
    const search = (text: string): boolean => {
      typed.current = { text, at: event.timeStamp };
      // The same letter again moves on to the next match; more letters refine the current one.
      const repeated = [...text].every((char) => startsWith(char, text.charAt(0)));
      const query = repeated ? text.charAt(0) : text;
      const from = repeated ? index + 1 : index;
      for (let step = 0; step < items.length; step += 1) {
        const candidate = items[(from + step) % items.length];
        if (candidate && startsWith(candidate.text, query)) {
          focusRow(candidate.key);
          return true;
        }
      }
      return false;
    };
    // Nothing starts with the letters typed so far: a new search from this letter on.
    if (!(recent && search(typed.current.text + event.key))) search(event.key);
  };

  const onBlur = (event: FocusEvent<HTMLElement>) => {
    // Leaving the tree: the next Tab in lands on the current row again.
    if (!(event.relatedTarget instanceof Node && tree.current?.contains(event.relatedTarget))) setFocusKey(null);
  };

  const ref = useCallback((node: HTMLElement | null) => {
    tree.current = node;
  }, []);

  return {
    /** Spread on the `role="tree"` element. */
    treeProps: {
      role: "tree" as const,
      ref,
      onKeyDown,
      onBlur,
    },
    /** Spread on each row (`role="treeitem"`, with `treeItemAria`). */
    itemProps: (key: string) => ({
      tabIndex: key === tabStop ? 0 : -1,
      "data-tree-key": key,
      onFocus: () => {
        setFocusKey(key);
      },
      // A right click opens the same menu (and not the web view's own).
      onContextMenu: (event: MouseEvent) => {
        event.preventDefault();
        openMenu(key);
      },
    }),
    /** Moves the focus to a row (after its menu or a picker of it closed). */
    focus: (key: string) => {
      focusRow(key);
    },
    /** Keeps the focus on the row while it moves to another place (reordering). */
    keepFocus: (key: string) => {
      keep.current = key;
    },
  };
}
