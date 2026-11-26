import { act, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ipc } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { mockCore } from "./support/core";
import { renderApp } from "./support/render";
import { appStylesheet, type Environment } from "./support/styles";

/*
 * Stage 22 (decision 0079), the Windows 11 look. The shell tells each window whether Mica lies
 * behind it (`getWindowBackdrop`); with Mica the page leaves itself and its chrome transparent
 * (frame, icon rail, docked folder column), never the list, the reading pane or what lies over
 * the content, and only while the system allows transparency and does not force its colours.
 * How it looks on Windows itself cannot be seen here: Windows has to be checked by a person
 * (docs/testing/windows-checklist.md).
 */

const WINDOWS = { version: "0.1.3", commit: null, buildDate: null, platform: "windows" } as const;

/** The frame with the folder column docked beside the content. */
async function frame() {
  await screen.findByRole("main");
  act(() => useUiStore.setState({ sidebarExpanded: true, sidebarNarrow: false }));
  await screen.findByTestId("sidebar-column");
  return {
    html: document.documentElement,
    body: document.body,
    shell: document.querySelector<HTMLElement>("[data-sidebar]"),
    column: screen.queryByTestId("sidebar-column"),
    rail: screen.queryByTestId("icon-rail"),
    main: screen.getByRole("main"),
  };
}

async function background(element: Element | null, environment: Environment = {}) {
  if (!element) throw new Error("element missing");
  return (await appStylesheet()).resolved(element, "background-color", environment);
}

describe("window backdrop", () => {
  it("asks the shell with the typed command", async () => {
    const core = mockCore({ windowBackdrop: "mica" });
    await expect(ipc.getWindowBackdrop()).resolves.toBe("mica");
    expect(core.calls.map((c) => c.cmd)).toEqual(["get_window_backdrop"]);
  });

  it("with Mica the page and its chrome are transparent, the content stays opaque", async () => {
    mockCore({ appInfo: WINDOWS, windowBackdrop: "mica" });
    renderApp();
    const { html, body, shell, column, main } = await frame();
    await waitFor(() => expect(html.dataset["backdrop"]).toBe("mica"));
    expect(column?.hasAttribute("data-chrome")).toBe(true);
    for (const element of [html, body, shell, column]) {
      expect(await background(element)).toBe("transparent");
    }
    expect(await background(main)).toBe("var(--ui-surface)");
  });

  it("the folded rail is chrome too", async () => {
    mockCore({ appInfo: WINDOWS, windowBackdrop: "mica" });
    renderApp();
    await frame();
    act(() => useUiStore.setState({ sidebarExpanded: false, sidebarNarrow: false }));
    const rail = await screen.findByTestId("icon-rail");
    expect(await background(rail)).toBe("transparent");
  });

  it("the column lying over the content in a narrow window stays opaque", async () => {
    mockCore({ appInfo: WINDOWS, windowBackdrop: "mica" });
    renderApp();
    await frame();
    act(() => useUiStore.setState({ sidebarNarrow: true, sidebarOverlay: true }));
    const over = await waitFor(() => {
      const column = document.querySelector('[data-testid="sidebar-column"][data-overlay]');
      expect(column).not.toBeNull();
      return column;
    });
    expect(over?.hasAttribute("data-chrome")).toBe(false);
    expect(await background(over)).toBe("var(--ui-rail)");
  });

  it("transparency effects off or a contrast theme: every surface is opaque again", async () => {
    mockCore({ appInfo: WINDOWS, windowBackdrop: "mica" });
    renderApp();
    const { body, shell, column } = await frame();
    await waitFor(() => expect(document.documentElement.dataset["backdrop"]).toBe("mica"));
    for (const environment of [{ reducedTransparency: true }, { forcedColors: true }]) {
      expect(await background(body, environment)).toBe("var(--ui-canvas)");
      expect(await background(shell, environment)).toBe("var(--ui-canvas)");
      expect(await background(column, environment)).toBe("var(--ui-rail)");
    }
  });

  it("without Mica (macOS, Windows 10, other windows) nothing changes", async () => {
    mockCore({ appInfo: WINDOWS });
    renderApp();
    const { html, body, shell, column } = await frame();
    expect(html.dataset["backdrop"]).toBeUndefined();
    expect(await background(body)).toBe("var(--ui-canvas)");
    expect(await background(shell)).toBe("var(--ui-canvas)");
    expect(await background(column)).toBe("var(--ui-rail)");
  });
});
