// Stage 16: the list of blocked senders in Settings → Inbox, with "Unblock", the empty state,
// typed errors, and a reload when another device (or the reading pane) changes the list.
import { act, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { parityIpc } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { ipcError, makeAccount, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { focusRingReport } from "./support/focusRing";
import { renderApp } from "./support/render";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

const ADA = makeAccount({ writesEnabled: true });
const BLOCKED = [
  { address: "promo@shop.example", createdAt: Date.UTC(2026, 10, 20, 9, 0) },
  { address: "spam@bulk.example", createdAt: Date.UTC(2026, 10, 21, 9, 0) },
];

function setup(options: Parameters<typeof mockCore>[0] = {}) {
  const core = mockCore({ accounts: [ADA], mailboxes: standardMailboxes(), parity: { blockedSenders: BLOCKED }, ...options });
  const rendered = renderApp();
  act(() => {
    useUiStore.getState().showSettings("inbox");
  });
  return { core, ...rendered };
}

describe("blocked senders", () => {
  it("lists them with the date and unblocks one", async () => {
    const { core, user } = setup();
    const list = await screen.findByRole("list", { name: "Blocked senders" });
    const rows = within(list).getAllByTestId("blocked-sender");
    expect(rows.map((row) => row.textContent)).toEqual([
      expect.stringContaining("promo@shop.example"),
      expect.stringContaining("spam@bulk.example"),
    ]);
    expect(rows[0]?.textContent).toContain("Blocked on");

    await user.click(within(list).getByRole("button", { name: "Unblock promo@shop.example" }));
    await waitFor(() => {
      expect(core.callsOf("unblock_sender")).toEqual([{ address: "promo@shop.example" }]);
    });
    await waitFor(() => {
      expect(within(list).getAllByTestId("blocked-sender")).toHaveLength(1);
    });
    expect(core.parity.blockedSenders.map((b) => b.address)).toEqual(["spam@bulk.example"]);
  });

  it("says how to block one while the list is empty", async () => {
    setup({ parity: { blockedSenders: [] } });
    const empty = await screen.findByTestId("blocked-empty");
    expect(empty.textContent).toContain("Block sender");
  });

  it("shows a typed error with a way to retry when the list cannot be loaded", async () => {
    const { core, user } = setup({ failures: { list_blocked_senders: ipcError("database") } });
    const notice = await screen.findByTestId("blocked-load-error");
    expect(notice.getAttribute("data-error-code")).toBe("database");
    expect(notice.textContent).toContain("The blocked senders could not be loaded.");
    const before = core.callsOf("list_blocked_senders").length;
    await user.click(within(notice).getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(core.callsOf("list_blocked_senders").length).toBeGreaterThan(before);
    });
  });

  it("shows why an unblock failed", async () => {
    const { user } = setup({ failures: { unblock_sender: ipcError("database") } });
    const list = await screen.findByRole("list", { name: "Blocked senders" });
    await user.click(within(list).getByRole("button", { name: "Unblock spam@bulk.example" }));
    const notice = await screen.findByTestId("blocked-unblock-error");
    expect(notice.textContent).toContain("The sender could not be unblocked.");
    expect(within(list).getAllByTestId("blocked-sender")).toHaveLength(2);
  });

  it("reloads after inboxChanged (a sender blocked on another device)", async () => {
    const { core } = setup({ parity: { blockedSenders: [] } });
    await screen.findByTestId("blocked-empty");
    const blocked = core.callsOf("list_blocked_senders").length;
    // Another device blocked a sender; the settings sync applied it in the core, which says so.
    await parityIpc.blockSender("new@spam.example");
    await emitCore({ type: "inboxChanged", accountId: null });
    await waitFor(() => {
      expect(core.callsOf("list_blocked_senders").length).toBeGreaterThan(blocked);
    });
    const list = await screen.findByRole("list", { name: "Blocked senders" });
    expect(list.textContent).toContain("new@spam.example");
  });

  it("says it in German", async () => {
    mockCore({ preferences: { theme: "system", language: "de" }, accounts: [ADA], parity: { blockedSenders: BLOCKED } });
    renderApp("de");
    act(() => {
      useUiStore.getState().showSettings("inbox");
    });
    const list = await screen.findByRole("list", { name: "Blockierte Absender" });
    expect(within(list).getByRole("button", { name: "promo@shop.example freigeben" })).toBeTruthy();
  });

  describe.each(["light", "dark"] as const)("%s theme", (theme) => {
    it("has no serious axe findings and focus rings", async () => {
      setup({ preferences: { theme, language: "system" } });
      const list = await screen.findByRole("list", { name: "Blocked senders" });
      const results = await axe.run(document.body, {
        resultTypes: ["violations"],
        iframes: false,
        rules: { "color-contrast": { enabled: false } },
      });
      expect(results.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => v.id)).toEqual([]);
      const report = await focusRingReport(list);
      expect(report.findings).toEqual([]);
      expect(report.checked).toBe(2);
    });
  });
});
