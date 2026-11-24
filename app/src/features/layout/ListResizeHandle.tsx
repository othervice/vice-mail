import { useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ResizeHandle, textScale } from "../../components/ResizeHandle";
import { useLayoutStore } from "../../stores/layout";
import { LIST_HEIGHT, LIST_WIDTH, useUiStore } from "../../stores/ui";
import type { ReadingLayout } from "./useReadingLayout";

/** A length from the stylesheet in px at 100 % text size ("22.5rem" is 360); 0 when unset. */
function lengthAt100(value: string, scale: number): number {
  const number = Number.parseFloat(value);
  if (!Number.isFinite(number)) return 0;
  return value.trim().endsWith("rem") ? number * 16 : number / scale;
}

/**
 * Stage 18 (docs/DESIGN.md, "Lesebereich"): the handle on the border between the message list
 * and the reading pane, the same as the sidebar's. With the pane on the right it sets the list's
 * width, with the pane below its height; in place of the list there is no border. It lies
 * between list and pane in their box (`layout.container`) and takes no room there.
 *
 * The size is stored (`WindowLayout.listWidth`, `listHeight`) and set as a variable on that box
 * through the CSSOM (CSP: no style attributes). The stylesheet keeps the pane at least
 * `--ui-reading-min-width` wide (`--ui-reading-min-height` high): in a window that became too
 * small the list is drawn smaller and the stored value stays. The handle measures the same room,
 * so its value, End and a drag start from what shows.
 */
export function ListResizeHandle({ layout, open }: { layout: ReadingLayout; open: boolean }) {
  if (!open || layout.position === "replace") return null;
  return <ListSplitter key={layout.position} vertical={layout.position === "right"} controls={layout.listId} />;
}

function ListSplitter({ vertical, controls }: { vertical: boolean; controls: string }) {
  const { t } = useTranslation();
  const stored = useUiStore((state) => (vertical ? state.listWidth : state.listHeight));
  const setListWidth = useUiStore((state) => state.setListWidth);
  const setListHeight = useUiStore((state) => state.setListHeight);
  const textSize = useLayoutStore((state) => state.textSize);
  const limits = vertical ? LIST_WIDTH : LIST_HEIGHT;
  const handle = useRef<HTMLDivElement>(null);
  // The largest list that leaves the pane its minimum; null until measured (no layout in tests).
  const [room, setRoom] = useState<number | null>(null);

  useLayoutEffect(() => {
    const box = handle.current?.parentElement;
    if (!box) return undefined;
    box.style.setProperty(vertical ? "--ui-list-width-reading" : "--ui-list-height-reading", `${String(stored / 16)}rem`);
    return () => {
      box.style.removeProperty(vertical ? "--ui-list-width-reading" : "--ui-list-height-reading");
    };
  }, [stored, vertical]);

  useLayoutEffect(() => {
    const box = handle.current?.parentElement;
    if (!box) return undefined;
    const measure = () => {
      const rect = box.getBoundingClientRect();
      const size = vertical ? rect.width : rect.height;
      if (size <= 0) {
        setRoom(null);
        return;
      }
      const scale = textScale();
      const paneMin = lengthAt100(getComputedStyle(box).getPropertyValue(vertical ? "--ui-reading-min-width" : "--ui-reading-min-height"), scale);
      setRoom(Math.floor(size / scale - paneMin));
    };
    measure();
    if (typeof ResizeObserver === "undefined") return undefined;
    const observer = new ResizeObserver(measure);
    observer.observe(box);
    return () => {
      observer.disconnect();
    };
  }, [vertical, textSize]);

  const fit = { ...limits, max: room === null ? limits.max : Math.min(limits.max, Math.max(limits.min, room)) };
  const shown = Math.min(stored, fit.max);
  return (
    <ResizeHandle
      ref={handle}
      orientation={vertical ? "vertical" : "horizontal"}
      value={shown}
      limits={fit}
      onChange={vertical ? setListWidth : setListHeight}
      label={t(vertical ? "layout.listSize.width" : "layout.listSize.height")}
      valueText={t("layout.listSize.value", { size: shown })}
      controls={controls}
      // No room of its own: it straddles the list's hairline, half over the list, half over the pane.
      className={vertical ? "relative -mx-1 shrink-0" : "relative -my-1 shrink-0"}
      data-testid="list-resize"
    />
  );
}
