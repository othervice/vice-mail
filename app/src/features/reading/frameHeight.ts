import { useLayoutEffect, type RefObject } from "react";

/**
 * Sets the height of a mail frame to the height of its content, so the reading pane scrolls as
 * a whole and the frame never shows a scroll bar of its own.
 *
 * The frame is `sandbox="allow-same-origin"`: the app may read its document, but no script runs
 * in it, and under WebKit no event listener on its nodes is ever called (decision 0005). What
 * does work, checked under WKWebView on macOS 27.2 (stage 3): the `load` event of the iframe
 * element (it belongs to the app's document), reading `contentDocument`, and a ResizeObserver
 * created here that observes the frame's root element. It reported the content height after
 * the load, after the frame became narrower and after a `<details>` in the mail was opened.
 *
 * Measuring: the frame is set to 0 px for a moment and the root's `scrollHeight` read, then the
 * height set. Reading the height at full size would never let it shrink (`scrollHeight` of the
 * root is at least the frame's own height), for example when the pane gets wider. Both steps
 * run in one task, so nothing is painted in between; the pane's scroll position is restored in
 * case the browser clamped it while the content was short.
 *
 * If the document cannot be read (a future opaque frame, decision 0005 "Rückweg"), the frame
 * gets the height of the pane and scrolls by itself.
 */
export function measureFrame(frame: HTMLIFrameElement, scroller: HTMLElement | null): number {
  const root = readableRoot(frame);
  if (!root) {
    const height = Math.max(0, scroller?.clientHeight ?? 0);
    frame.style.height = `${String(height)}px`;
    return height;
  }
  const top = scroller?.scrollTop ?? 0;
  frame.style.height = "0px";
  const height = Math.max(0, Math.ceil(root.scrollHeight));
  frame.style.height = `${String(height)}px`;
  if (scroller && scroller.scrollTop !== top) scroller.scrollTop = top;
  return height;
}

function readableRoot(frame: HTMLIFrameElement): HTMLElement | null {
  try {
    return frame.contentDocument?.documentElement ?? null;
  } catch {
    // An opaque frame throws or returns null, depending on the engine.
    return null;
  }
}

/**
 * Keeps the frame as high as its content: measured right on `load`, and again when its content
 * changes size or the pane changes width (bursts merged into one measurement).
 *
 * The measurement runs as a task right after the ResizeObserver callback: inside the callback
 * it would change observed sizes during delivery ("ResizeObserver loop" errors). Not in an
 * animation frame: WebKit runs none while the window is hidden or covered, while ResizeObserver
 * keeps reporting (seen in the stage 3 check under WKWebView), so the height would only follow
 * once the window shows again.
 */
export function useFrameHeight(
  frameRef: RefObject<HTMLIFrameElement | null>,
  scrollerRef: RefObject<HTMLElement | null>,
): void {
  // A layout effect: the listener must be in place before the frame's first `load`, which is
  // queued as soon as React inserts the iframe.
  useLayoutEffect(() => {
    const frame = frameRef.current;
    if (!frame) return undefined;
    let pending: ReturnType<typeof setTimeout> | null = null;
    let contentObserver: ResizeObserver | null = null;
    let paneWidth = -1;

    const measure = () => {
      pending = null;
      measureFrame(frame, scrollerRef.current);
    };
    const schedule = () => {
      pending ??= setTimeout(measure, 0);
    };
    const onLoad = () => {
      contentObserver?.disconnect();
      contentObserver = null;
      measure();
      const root = readableRoot(frame);
      if (root) {
        contentObserver = new ResizeObserver(schedule);
        contentObserver.observe(root);
      }
    };
    // Not the frame itself: its height is what this sets, its width follows the pane.
    const paneObserver = new ResizeObserver((entries) => {
      const width = entries[entries.length - 1]?.contentRect.width ?? -1;
      if (width === paneWidth) return;
      paneWidth = width;
      schedule();
    });
    const scroller = scrollerRef.current;
    if (scroller) paneObserver.observe(scroller);

    frame.addEventListener("load", onLoad);
    return () => {
      frame.removeEventListener("load", onLoad);
      paneObserver.disconnect();
      contentObserver?.disconnect();
      if (pending !== null) clearTimeout(pending);
    };
  }, [frameRef, scrollerRef]);
}
