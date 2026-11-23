import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import printCss from "../src/features/extras/print.css?raw";
import tokensCss from "../src/styles/tokens.css?raw";
import { APP_INFO, ipcError, makeAccount, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { NO_UNSUBSCRIBE } from "./support/extras";
import { renderApp } from "./support/render";

const ADA = makeAccount();

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

const MAIL = makeMessage({ id: 1_700_000_300, subject: "Quarterly report" });

type Options = NonNullable<Parameters<typeof mockCore>[0]>;

function setup(extra: Options = {}) {
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: [MAIL] },
    views: { [MAIL.id]: makeView(MAIL) },
    readingPreferences: { markReadOnOpen: "never" },
    ...extra,
  });
  return { core, ...renderApp() };
}

async function open(user: ReturnType<typeof renderApp>["user"]) {
  const [option] = await screen.findAllByRole("option");
  if (!option) throw new Error("no row");
  await user.click(option);
  const pane = await screen.findByRole("region", { name: "Message" });
  await within(pane).findByRole("heading", { level: 2, name: "Quarterly report" });
  return pane;
}

const tools = (pane: HTMLElement) => within(pane).getByRole("group", { name: "Export and print" });

describe("export as .eml", () => {
  it("asks the core (which shows the save dialog) and says where the file went", async () => {
    const { core, user } = setup({ extras: { exportPath: "/Users/ada/Desktop/Quarterly report.eml" } });
    const pane = await open(user);
    await user.click(within(tools(pane)).getByRole("button", { name: "Save as .eml file" }));
    const saved = await within(pane).findByTestId("export-saved");
    expect(saved.getAttribute("role")).toBe("status");
    expect(saved.textContent).toContain("Saved as /Users/ada/Desktop/Quarterly report.eml");
    expect(core.callsOf("export_message")).toEqual([{ messageId: MAIL.id }]);
    await user.click(within(saved).getByRole("button", { name: "Close" }));
    expect(within(pane).queryByTestId("export-saved")).toBeNull();
  });

  it("a cancelled dialog shows nothing", async () => {
    const { core, user } = setup({ extras: { exportPath: null } });
    const pane = await open(user);
    await user.click(within(tools(pane)).getByRole("button", { name: "Save as .eml file" }));
    await waitFor(() => {
      expect(core.callsOf("export_message")).toHaveLength(1);
    });
    expect(within(pane).queryByTestId("export-saved")).toBeNull();
    expect(within(pane).queryByTestId("tools-error")).toBeNull();
  });

  it("a failure is shown with title and hint", async () => {
    const { user } = setup({ failures: { export_message: ipcError("connection_lost") } });
    const pane = await open(user);
    await user.click(within(tools(pane)).getByRole("button", { name: "Save as .eml file" }));
    const error = await within(pane).findByTestId("tools-error");
    expect(error.dataset["errorCode"]).toBe("connection_lost");
    expect(error.textContent).toContain("The message could not be saved");
  });
});

describe("printing", () => {
  it("the button opens the system's print dialog", async () => {
    const { core, user } = setup();
    const pane = await open(user);
    await user.click(within(tools(pane)).getByRole("button", { name: "Print or save as PDF (⌘P)" }));
    await waitFor(() => {
      expect(core.extras.state.prints).toBe(1);
    });
  });

  it("Cmd+P on macOS prints the message, Ctrl+P does not", async () => {
    const { core, user } = setup();
    await open(user);
    fireEvent.keyDown(window, { key: "p", ctrlKey: true });
    fireEvent.keyDown(window, { key: "p", metaKey: true });
    await waitFor(() => {
      expect(core.extras.state.prints).toBe(1);
    });
  });

  it("Ctrl+P on Windows", async () => {
    const { core, user } = setup({ appInfo: { ...APP_INFO, platform: "windows" } });
    const pane = await open(user);
    expect(within(tools(pane)).getByRole("button", { name: "Print or save as PDF (Ctrl+P)" })).toBeTruthy();
    fireEvent.keyDown(window, { key: "p", ctrlKey: true });
    await waitFor(() => {
      expect(core.extras.state.prints).toBe(1);
    });
  });

  it("does not print from inside the composer", async () => {
    const { core, user } = setup();
    await open(user);
    await user.click(screen.getByRole("button", { name: "New message" }));
    const subject = await screen.findByLabelText("Subject");
    fireEvent.keyDown(subject, { key: "p", metaKey: true });
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(core.extras.state.prints).toBe(0);
  });

  it("a failure to open the dialog is shown", async () => {
    const { user } = setup({ failures: { print_window: ipcError("internal", "print: no printer support") } });
    const pane = await open(user);
    await user.click(within(tools(pane)).getByRole("button", { name: /Print or save as PDF/ }));
    expect((await within(pane).findByTestId("tools-error")).textContent).toContain("The print dialog could not be opened");
  });

  it("marks the message as what is printed, and its controls as not printed", async () => {
    const { user } = setup({
      extras: { extras: { [MAIL.id]: { unsubscribe: { ...NO_UNSUBSCRIBE, oneClickHost: "lists.example.org" } } } },
    });
    const pane = await open(user);
    const article = within(pane).getByRole("article");
    expect(article.hasAttribute("data-print-root")).toBe(true);
    // Only the message is printed: the top bar with actions and tools is outside it.
    expect(article.contains(tools(pane))).toBe(false);
    const bar = await within(pane).findByTestId("unsubscribe-bar");
    expect(bar.hasAttribute("data-print-hide")).toBe(true);
    expect(within(pane).getByTestId("category-panel").closest("[data-print-hide]")).not.toBeNull();
  });
});

/** `--name: #hex;` pairs of the first `selector { … }` block in a stylesheet. */
function tokens(css: string, selector: string): Map<string, string> {
  const start = css.indexOf(`${selector} {`);
  const block = css.slice(start, css.indexOf("}", start));
  return new Map([...block.matchAll(/(--ui-[a-z0-9-]+):\s*(#[0-9a-f]{6});/g)].map((m) => [m[1] ?? "", m[2] ?? ""]));
}

describe("print styles", () => {
  it("print only the message and hide its controls", () => {
    expect(printCss).toContain("@media print");
    expect(printCss).toMatch(/body \*:not\(\[data-print-root\], \[data-print-root\] \*, :has\(\[data-print-root\]\)\)\s*\{\s*display: none !important;/);
    expect(printCss).toMatch(/\[data-print-root\] \[data-print-hide\],\s*\[data-print-root\] button\[aria-label\]\s*\{\s*display: none !important;/);
    // The ancestors of the message stop clipping it at the window's height.
    expect(printCss).toMatch(/body \*:has\(\[data-print-root\]\)\s*\{[^}]*overflow: visible !important;[^}]*height: auto !important;/);
  });

  it("print the dark theme with exactly the light theme's colours", () => {
    const light = tokens(tokensCss, ":root");
    const print = tokens(printCss, ':root[data-theme="dark"]');
    expect(print.size).toBeGreaterThan(5);
    for (const [name, value] of print) expect(`${name}: ${value}`).toBe(`${name}: ${light.get(name) ?? "missing"}`);
  });
});
