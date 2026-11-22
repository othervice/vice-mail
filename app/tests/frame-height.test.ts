import { describe, expect, it } from "vitest";
import { measureFrame } from "../src/features/reading/frameHeight";

/**
 * A frame whose document behaves like WebKit's: the root's `scrollHeight` is at least the
 * frame's own height, and the content is `content` px high.
 */
function fakeFrame(content: () => number, scroller?: { scrollTop: number; clientHeight: number }) {
  const frame = document.createElement("iframe");
  const ownHeight = () => Number.parseFloat(frame.style.height || "150");
  const root = {
    get scrollHeight() {
      // While the content is short, the browser clamps the pane's scroll position.
      if (scroller && ownHeight() === 0) scroller.scrollTop = 0;
      return Math.max(ownHeight(), content());
    },
  };
  Object.defineProperty(frame, "contentDocument", { configurable: true, get: () => ({ documentElement: root }) });
  return frame;
}

describe("mail frame height", () => {
  it("is the height of the content", () => {
    const frame = fakeFrame(() => 812.4);
    expect(measureFrame(frame, null)).toBe(813);
    expect(frame.style.height).toBe("813px");
  });

  it("shrinks when the content gets shorter (a wider pane), which reading at full size never would", () => {
    let content = 2000;
    const frame = fakeFrame(() => content);
    measureFrame(frame, null);
    expect(frame.style.height).toBe("2000px");

    content = 900;
    measureFrame(frame, null);

    expect(frame.style.height).toBe("900px");
  });

  it("keeps the reader's place in the pane while measuring", () => {
    const scroller = { scrollTop: 640, clientHeight: 500 };
    const frame = fakeFrame(() => 3000, scroller);

    measureFrame(frame, scroller as unknown as HTMLElement);

    expect(frame.style.height).toBe("3000px");
    expect(scroller.scrollTop).toBe(640);
  });

  it("falls back to the pane's height, scrolling inside the frame, if the document cannot be read", () => {
    const frame = document.createElement("iframe");
    Object.defineProperty(frame, "contentDocument", { configurable: true, get: () => null });
    const scroller = { scrollTop: 0, clientHeight: 480 } as unknown as HTMLElement;

    expect(measureFrame(frame, scroller)).toBe(480);
    expect(frame.style.height).toBe("480px");
  });

  it("treats a frame that throws on access like an unreadable one", () => {
    const frame = document.createElement("iframe");
    Object.defineProperty(frame, "contentDocument", {
      configurable: true,
      get: () => {
        throw new DOMException("Blocked a frame", "SecurityError");
      },
    });
    expect(measureFrame(frame, null)).toBe(0);
  });
});
