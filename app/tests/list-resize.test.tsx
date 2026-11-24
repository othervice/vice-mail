import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import axe from "axe-core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import tokensCss from "../src/styles/tokens.css?raw";
import { DEFAULT_LIST_HEIGHT, DEFAULT_LIST_WIDTH, MAX_LIST_WIDTH, MIN_LIST_HEIGHT, MIN_LIST_WIDTH, type CoreWindowLayout } from "../src/services/ipc";
import { useLayoutStore } from "../src/stores/layout";
import { LIST_HEIGHT, LIST_WIDTH, useUiStore } from "../src/stores/ui";
import { makeAccount, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";
import { DEFAULT_WINDOW_LAYOUT } from "./support/uiPrefs";

/*
 * Stage 18, task A (docs/DESIGN.md, "Lesebereich"): the border between the message list and the
 * reading pane is a window splitter like the sidebar's. Right of the list it sets the list's
 * width, below it the list's height; in place of the list there is none. The size is stored with
 * the window's layout and the pane keeps a minimum when the window gets smaller.
 */

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  // jsdom has no pointer capture.
  HTMLElement.prototype.setPointerCapture = vi.fn();
});

afterEach(() => {
  Reflect.deleteProperty(HTMLElement.prototype, "setPointerCapture");
});

function setup(layout: Partial<CoreWindowLayout> = {}) {
  const items = makeMessages(4);
  const core = mockCore({
    ui: { layout: { ...DEFAULT_WINDOW_LAYOUT, ...layout } },
    accounts: [makeAccount({ writesEnabled: true })],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
  });
  return { core, items, ...renderApp() };
}

/** Opens the first message: the list selects it as soon as it has the focus. */
async function openFirst() {
  const list = await screen.findByRole("listbox", { name: /^Messages in/ });
  act(() => {
    list.focus();
  });
  await screen.findByRole("region", { name: "Message" });
  return list;
}

const box = () => document.querySelector<HTMLElement>("[data-reading-pane]");
const listColumn = (list: HTMLElement) => list.parentElement as HTMLElement;

describe("the border between list and reading pane", () => {
  it("is a vertical splitter with the list's width while the pane is on the right", async () => {
    const { core } = setup();
    // Stage 18 (task F): the border is there as soon as the list is, open message or not.
    const list = await openFirst();
    const splitter = screen.getByRole("separator", { name: "List width" });
    expect(splitter.getAttribute("aria-orientation")).toBe("vertical");
    expect(splitter.getAttribute("aria-valuenow")).toBe(String(DEFAULT_LIST_WIDTH));
    expect(splitter.getAttribute("aria-valuemin")).toBe(String(MIN_LIST_WIDTH));
    expect(splitter.getAttribute("aria-valuetext")).toBe("384 pixels");
    expect(splitter.tabIndex).toBe(0);
    // It names the list column it resizes and lies between list and pane.
    const column = listColumn(list);
    expect(splitter.getAttribute("aria-controls")).toBe(column.id);
    expect(column.nextElementSibling).toBe(splitter);
    expect(column.className).toContain("w-(--ui-list-width-reading)");
    expect(column.className).toContain("max-w-[calc(100%-var(--ui-reading-min-width))]");
    expect(column.className).toContain("min-w-(--ui-list-min-width-reading)");
    expect(box()?.style.getPropertyValue("--ui-list-width-reading")).toBe("24rem");

    // Arrow keys by 16 px, Home and End to the limits; the box follows through the CSSOM.
    act(() => {
      splitter.focus();
    });
    fireEvent.keyDown(splitter, { key: "ArrowRight" });
    expect(splitter.getAttribute("aria-valuenow")).toBe("400");
    expect(box()?.style.getPropertyValue("--ui-list-width-reading")).toBe("25rem");
    fireEvent.keyDown(splitter, { key: "ArrowLeft" });
    fireEvent.keyDown(splitter, { key: "ArrowLeft" });
    expect(splitter.getAttribute("aria-valuenow")).toBe("368");
    fireEvent.keyDown(splitter, { key: "Home" });
    expect(splitter.getAttribute("aria-valuenow")).toBe(String(MIN_LIST_WIDTH));
    fireEvent.keyDown(splitter, { key: "ArrowLeft" });
    expect(splitter.getAttribute("aria-valuenow")).toBe(String(MIN_LIST_WIDTH));
    fireEvent.keyDown(splitter, { key: "End" });
    expect(splitter.getAttribute("aria-valuenow")).toBe(String(MAX_LIST_WIDTH));
    // ↑ and ↓ belong to a horizontal splitter; Enter does nothing here (the sidebar's folds).
    fireEvent.keyDown(splitter, { key: "ArrowDown" });
    fireEvent.keyDown(splitter, { key: "Enter" });
    expect(splitter.getAttribute("aria-valuenow")).toBe(String(MAX_LIST_WIDTH));
    expect(screen.getByRole("region", { name: "Message" })).toBeTruthy();

    // A double click sets the default again; a rest of the size is one save with the window.
    fireEvent.doubleClick(splitter);
    expect(splitter.getAttribute("aria-valuenow")).toBe(String(DEFAULT_LIST_WIDTH));
    fireEvent.keyDown(splitter, { key: "ArrowRight" });
    fireEvent.keyDown(splitter, { key: "ArrowRight" });
    await waitFor(() => {
      expect(core.ui.layout.listWidth).toBe(416);
    });
    const saved = core.callsOf("set_window_layout").map((call) => (call as { layout: CoreWindowLayout }).layout);
    expect(saved).toHaveLength(1);
    expect(saved[0]).toEqual({ ...DEFAULT_WINDOW_LAYOUT, listWidth: 416 });
    expect(useLayoutStore.getState().listWidth).toBe(416);
  });

  it("follows the pointer: a drag changes the width by the way the pointer went", async () => {
    setup();
    await openFirst();
    const splitter = screen.getByRole("separator", { name: "List width" });
    fireEvent.pointerDown(splitter, { button: 0, pointerId: 1, clientX: 400, clientY: 300 });
    expect(splitter.setPointerCapture).toHaveBeenCalledWith(1);
    fireEvent.pointerMove(splitter, { pointerId: 1, clientX: 480, clientY: 310 });
    expect(splitter.getAttribute("aria-valuenow")).toBe("464");
    fireEvent.pointerMove(splitter, { pointerId: 1, clientX: 100, clientY: 310 });
    // Never below the list's minimum.
    expect(splitter.getAttribute("aria-valuenow")).toBe(String(MIN_LIST_WIDTH));
    fireEvent.pointerMove(splitter, { pointerId: 1, clientX: 450, clientY: 310 });
    expect(splitter.getAttribute("aria-valuenow")).toBe("434");
    fireEvent.pointerUp(splitter, { pointerId: 1, clientX: 450 });
    // After the button is up, moving does nothing.
    fireEvent.pointerMove(splitter, { pointerId: 1, clientX: 600 });
    expect(splitter.getAttribute("aria-valuenow")).toBe("434");
    expect(useUiStore.getState().listWidth).toBe(434);
  });

  it("is a horizontal splitter with the list's height while the pane is below", async () => {
    const { core } = setup({ readingPane: "bottom" });
    const list = await openFirst();
    expect(screen.queryByRole("separator", { name: "List width" })).toBeNull();
    const splitter = screen.getByRole("separator", { name: "List height" });
    expect(splitter.getAttribute("aria-orientation")).toBe("horizontal");
    expect(splitter.getAttribute("aria-valuenow")).toBe(String(DEFAULT_LIST_HEIGHT));
    expect(splitter.getAttribute("aria-controls")).toBe(listColumn(list).id);
    expect(listColumn(list).className).toContain("h-(--ui-list-height-reading)");
    expect(listColumn(list).className).toContain("max-h-[calc(100%-var(--ui-reading-min-height))]");
    expect(box()?.style.getPropertyValue("--ui-list-height-reading")).toBe("20rem");

    fireEvent.keyDown(splitter, { key: "ArrowDown" });
    expect(splitter.getAttribute("aria-valuenow")).toBe("336");
    fireEvent.keyDown(splitter, { key: "ArrowUp" });
    fireEvent.keyDown(splitter, { key: "ArrowUp" });
    expect(splitter.getAttribute("aria-valuenow")).toBe("304");
    fireEvent.keyDown(splitter, { key: "ArrowRight" });
    expect(splitter.getAttribute("aria-valuenow")).toBe("304");
    fireEvent.keyDown(splitter, { key: "Home" });
    expect(splitter.getAttribute("aria-valuenow")).toBe(String(MIN_LIST_HEIGHT));
    fireEvent.pointerDown(splitter, { button: 0, pointerId: 2, clientX: 300, clientY: 200 });
    fireEvent.pointerMove(splitter, { pointerId: 2, clientX: 900, clientY: 300 });
    expect(splitter.getAttribute("aria-valuenow")).toBe("260");
    fireEvent.pointerUp(splitter, { pointerId: 2 });
    expect(box()?.style.getPropertyValue("--ui-list-height-reading")).toBe("16.25rem");
    await waitFor(() => {
      expect(core.ui.layout.listHeight).toBe(260);
    });
    // The width beside the pane is a size of its own and stays.
    expect(core.ui.layout.listWidth).toBe(DEFAULT_LIST_WIDTH);
  });

  it("does not exist while the message shows in place of the list", async () => {
    const { user } = setup({ readingPane: "replace" });
    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    list.focus();
    await user.keyboard("{Enter}");
    await screen.findByRole("region", { name: "Message" });
    expect(screen.queryByRole("separator", { name: /^List/ })).toBeNull();
  });

  it("opens at the stored size and follows a stored change", async () => {
    setup({ listWidth: 500, listHeight: 400 });
    await openFirst();
    const splitter = screen.getByRole("separator", { name: "List width" });
    expect(splitter.getAttribute("aria-valuenow")).toBe("500");
    expect(box()?.style.getPropertyValue("--ui-list-width-reading")).toBe("31.25rem");
    // Another window, a restart or a refused save puts a stored size in place.
    act(() => {
      useLayoutStore.getState().applyStoredLayout({ ...DEFAULT_WINDOW_LAYOUT, listWidth: 640, listHeight: 400 });
    });
    expect(splitter.getAttribute("aria-valuenow")).toBe("640");
    act(() => {
      useLayoutStore.getState().setLayout({ readingPane: "bottom" });
    });
    expect(screen.getByRole("separator", { name: "List height" }).getAttribute("aria-valuenow")).toBe("400");
  });

  it("leaves the pane its minimum in a small window and keeps the stored width", async () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      const width = this.hasAttribute("data-reading-pane") ? 1000 : 0;
      return { x: 0, y: 0, top: 0, left: 0, right: width, bottom: 600, width, height: 600, toJSON: () => ({}) };
    });
    // The pane's minimum comes from the stylesheet (tokens.css), which jsdom does not load.
    const original = window.getComputedStyle.bind(window);
    vi.spyOn(window, "getComputedStyle").mockImplementation((element: Element, pseudo?: string | null) => {
      const style = original(element, pseudo);
      if (!(element instanceof HTMLElement) || !element.hasAttribute("data-reading-pane")) return style;
      return new Proxy(style, {
        get: (target, key) =>
          key === "getPropertyValue"
            ? (name: string) => (name === "--ui-reading-min-width" ? "22.5rem" : target.getPropertyValue(name))
            : (Reflect.get(target, key) as unknown),
      });
    });
    const { core } = setup({ listWidth: 900 });
    await openFirst();
    const splitter = screen.getByRole("separator", { name: "List width" });
    // 1000 px for list and pane, at least 360 for the pane: the list shows at 640.
    expect(splitter.getAttribute("aria-valuemax")).toBe("640");
    expect(splitter.getAttribute("aria-valuenow")).toBe("640");
    expect(splitter.getAttribute("aria-valuetext")).toBe("640 pixels");
    // The stylesheet draws the same (the stored width, at most the room): the value stays.
    expect(box()?.style.getPropertyValue("--ui-list-width-reading")).toBe("56.25rem");
    expect(useUiStore.getState().listWidth).toBe(900);
    // The keys start from what shows.
    fireEvent.keyDown(splitter, { key: "ArrowLeft" });
    expect(splitter.getAttribute("aria-valuenow")).toBe("624");
    await waitFor(() => {
      expect(core.ui.layout.listWidth).toBe(624);
    });
    fireEvent.keyDown(splitter, { key: "End" });
    expect(splitter.getAttribute("aria-valuenow")).toBe("640");
  });

  it("has no serious accessibility findings", async () => {
    setup();
    await openFirst();
    expect(screen.getByRole("separator", { name: "List width" })).toBeTruthy();
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    const results = await axe.run(document.body, { resultTypes: ["violations"], iframes: false, rules: { "color-contrast": { enabled: false } } });
    const serious = results.violations
      .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
      .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
    expect(serious).toEqual([]);
  });

  it("uses the core's limits in the stylesheet", () => {
    const token = (name: string) => new RegExp(`${name}:\\s*([\\d.]+)rem;`).exec(tokensCss)?.[1];
    expect(Number(token("--ui-list-width-reading")) * 16).toBe(LIST_WIDTH.default);
    expect(Number(token("--ui-list-min-width-reading")) * 16).toBe(LIST_WIDTH.min);
    expect(Number(token("--ui-list-height-reading")) * 16).toBe(LIST_HEIGHT.default);
    expect(Number(token("--ui-list-min-height-reading")) * 16).toBe(LIST_HEIGHT.min);
    // The pane keeps more than the list's narrowest rows need.
    expect(Number(token("--ui-reading-min-width")) * 16).toBe(360);
    expect(Number(token("--ui-reading-min-height")) * 16).toBe(240);
  });
});
