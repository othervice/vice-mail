import { renderHook, screen, within } from "@testing-library/react";
import axe from "axe-core";
import { useRef } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useMenuPlacement, visibleBounds } from "../src/features/actions/useMenuPlacement";
import { makeAccount, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";

/*
 * Stage 21, item 3 (decision 0063): the menus of the accounts-and-folders column are as wide as the
 * column's rows, never wider, with names that wrap instead of being cut off; the column never
 * scrolls sideways; every menu stays inside what can be seen (above its button when there is no
 * room below, moved back in at the sides, smaller when neither side has room).
 */

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

const ADA = makeAccount({ writesEnabled: true, displayName: "Ada" });

function setup(account = ADA) {
  mockCore({ accounts: [account], mailboxes: standardMailboxes(account.id), messages: { 1: [], 5: [], 6: [] } });
  return renderApp();
}

type User = ReturnType<typeof renderApp>["user"];

async function openColumn(user: User) {
  if (screen.queryByTestId("sidebar-column")) return;
  await user.click(await screen.findByRole("button", { name: "Folders" }));
  await screen.findByTestId("sidebar-column");
}

describe("menus of the column", () => {
  it("are as wide as the column allows, with names that wrap instead of being cut off", async () => {
    const { user } = setup();
    await openColumn(user);
    await user.click(await screen.findByRole("button", { name: "Options for Projekte" }));
    const menu = await screen.findByRole("menu", { name: "Options for Projekte" });
    expect(menu.className).toContain("w-[min(var(--ui-sidebar-menu-width),calc(var(--ui-sidebar-width)-var(--ui-space-4)))]");
    expect(menu.className).not.toContain("min-w-");
    for (const item of within(menu).getAllByRole("menuitem")) {
      expect(item.className).toContain("min-h-8");
      expect(item.querySelector(".truncate")).toBeNull();
    }
    // The column's content clips sideways, and its scrolling area goes back if moved sideways.
    const scroll = screen.getByTestId("sidebar-scroll");
    expect(scroll.firstElementChild?.className).toContain("overflow-x-clip");
    scroll.scrollLeft = 40;
    scroll.dispatchEvent(new Event("scroll"));
    expect(scroll.scrollLeft).toBe(0);
  });

  it("have no serious accessibility findings, with an unavailable entry and its reason", async () => {
    const reader = makeAccount({ writesEnabled: false, displayName: "Leser" });
    const { user } = setup(reader);
    await openColumn(user);
    await user.click(await screen.findByRole("button", { name: "Options for Projekte" }));
    await screen.findByRole("menu", { name: "Options for Projekte" });
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    const results = await axe.run(document.body, { resultTypes: ["violations"], iframes: false, rules: { "color-contrast": { enabled: false } } });
    const serious = results.violations
      .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
      .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
    expect(serious).toEqual([]);
  });
});

/** A box as `getBoundingClientRect` gives it. */
function rect(left: number, top: number, width: number, height: number): DOMRect {
  return { left, top, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON: () => ({}) };
}

describe("menu placement", () => {
  function place(button: DOMRect, menu: DOMRect, clip?: { element: "x" | "y"; box: DOMRect }) {
    const outer = document.createElement("div");
    if (clip) outer.style.setProperty(clip.element === "x" ? "overflow-x" : "overflow-y", "hidden");
    const anchor = document.createElement("div");
    const list = document.createElement("div");
    outer.append(anchor);
    anchor.append(list);
    document.body.append(outer);
    vi.spyOn(anchor, "getBoundingClientRect").mockReturnValue(button);
    vi.spyOn(list, "getBoundingClientRect").mockReturnValue(menu);
    if (clip) vi.spyOn(outer, "getBoundingClientRect").mockReturnValue(clip.box);
    renderHook(() => {
      const anchorRef = useRef<HTMLElement | null>(anchor);
      const menuRef = useRef<HTMLElement | null>(list);
      useMenuPlacement(true, anchorRef, menuRef);
    });
    outer.remove();
    return list;
  }

  beforeEach(() => {
    vi.spyOn(document.documentElement, "clientWidth", "get").mockReturnValue(1000);
    vi.spyOn(document.documentElement, "clientHeight", "get").mockReturnValue(700);
  });

  it("opens below when there is room, above when there is not and more above", () => {
    expect(place(rect(100, 100, 32, 32), rect(0, 136, 200, 300)).getAttribute("data-side")).toBe("below");
    const up = place(rect(100, 600, 32, 32), rect(0, 636, 200, 300));
    expect(up.getAttribute("data-side")).toBe("above");
    expect(up.style.getPropertyValue("max-height")).toBe("");
  });

  it("gets a smaller height when neither side has room for all of it", () => {
    const tight = place(rect(100, 300, 32, 32), rect(0, 336, 200, 900));
    expect(tight.getAttribute("data-side")).toBe("below");
    expect(tight.style.getPropertyValue("max-height")).toBe(`${String(700 - 8 - (332 + 4))}px`);
  });

  it("moves back inside at the start and the end, within an area that clips it", () => {
    expect(place(rect(100, 100, 32, 32), rect(-60, 136, 192, 200)).style.getPropertyValue("translate")).toBe("68px");
    expect(place(rect(900, 100, 32, 32), rect(900, 136, 192, 200)).style.getPropertyValue("translate")).toBe("-100px");
    // A column 200 px wide that clips sideways: inside the column, not only the window.
    const column = { element: "x" as const, box: rect(48, 0, 200, 700) };
    expect(place(rect(200, 100, 32, 32), rect(40, 136, 192, 200), column).style.getPropertyValue("translate")).toBe("16px");
  });

  it("knows what can be seen: the window cut down by clipping ancestors", () => {
    const outer = document.createElement("div");
    outer.style.setProperty("overflow-y", "auto");
    const inner = document.createElement("div");
    outer.append(inner);
    document.body.append(outer);
    vi.spyOn(outer, "getBoundingClientRect").mockReturnValue(rect(0, 50, 300, 400));
    expect(visibleBounds(inner)).toEqual({ left: 0, top: 50, right: 1000, bottom: 450 });
    outer.remove();
  });
});
