import { useCallback, useRef, useState, type FocusEvent, type KeyboardEvent } from "react";

interface RovingFocusOptions {
  /** The entries in their order. */
  keys: readonly string[];
  /** The entry of what is shown right now: the tab stop when Tab comes in (else the first). */
  activeKey: string | null;
  orientation?: "vertical" | "horizontal";
}

function entryIn(group: HTMLElement | null, key: string): HTMLElement | undefined {
  return [...(group?.querySelectorAll<HTMLElement>("[data-roving-key]") ?? [])].find((entry) => entry.dataset["rovingKey"] === key);
}

/**
 * A group of buttons as one tab stop with a roving tabindex (WAI-ARIA toolbar): the arrow keys of
 * its orientation move between the entries, Home and End go to the first and last. Tab comes in on
 * the active entry. The entries stay native buttons, so Enter and Space work as on any button.
 */
export function useRovingFocus({ keys, activeKey, orientation = "vertical" }: RovingFocusOptions) {
  const [focusKey, setFocusKey] = useState<string | null>(null);
  const group = useRef<HTMLElement | null>(null);
  const tabStop =
    focusKey !== null && keys.includes(focusKey) ? focusKey : activeKey !== null && keys.includes(activeKey) ? activeKey : (keys[0] ?? null);

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    const target = event.target instanceof HTMLElement ? event.target : null;
    const key = target?.dataset["rovingKey"];
    if (event.defaultPrevented || key === undefined || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    const index = keys.indexOf(key);
    const [previous, next] = orientation === "vertical" ? ["ArrowUp", "ArrowDown"] : ["ArrowLeft", "ArrowRight"];
    const to =
      event.key === next ? keys[index + 1] : event.key === previous ? keys[index - 1] : event.key === "Home" ? keys[0] : event.key === "End" ? keys.at(-1) : null;
    if (to === null) return;
    event.preventDefault();
    if (to === undefined) return;
    setFocusKey(to);
    entryIn(group.current, to)?.focus();
  };

  const onBlur = (event: FocusEvent<HTMLElement>) => {
    if (!(event.relatedTarget instanceof Node && group.current?.contains(event.relatedTarget))) setFocusKey(null);
  };

  const ref = useCallback((node: HTMLElement | null) => {
    group.current = node;
  }, []);

  return {
    /** Spread on the `role="toolbar"` element. */
    groupProps: { ref, onKeyDown, onBlur, "aria-orientation": orientation },
    /** Spread on each entry. */
    itemProps: (key: string) => ({
      tabIndex: key === tabStop ? 0 : -1,
      "data-roving-key": key,
      onFocus: () => {
        setFocusKey(key);
      },
    }),
  };
}
