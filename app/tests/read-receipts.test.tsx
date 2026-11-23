import { act, screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Language } from "../src/i18n";
import type { ComposeSettings, MessageListItem, ReadReceiptInfo } from "../src/services/ipc";
import { useComposerStore } from "../src/stores/composer";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { optionName } from "./support/names";
import { makeReceiptInfo } from "./support/parityCompose";
import { renderApp } from "./support/render";

// Stage 16 (RFC 8098, decision 0032 point 2): a request for a read receipt is answered only by the
// user's click; "never" offers nothing; receipts that came back show at the sent message.

const ADA = makeAccount({ writesEnabled: true });
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  useComposerStore.setState({ session: null, openDraftId: null, sends: [], problems: [] });
});

interface SetupOptions {
  account?: typeof ADA;
  receipt?: Partial<ReadReceiptInfo>;
  settings?: ComposeSettings;
  language?: Language;
}

function setup({ account = ADA, receipt, settings, language = "en" }: SetupOptions = {}) {
  const items = makeMessages(2);
  const first = items[0] as MessageListItem;
  const core = mockCore({
    accounts: [account],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    preferences: { theme: "system", language },
    compose: { ...(settings && { settings }) },
    receipts: { receipts: receipt ? { [first.id]: makeReceiptInfo(receipt) } : {} },
  });
  return { core, first, ...renderApp(language) };
}

async function openMessage(user: UserEvent, subject: string) {
  const listbox = await screen.findByRole("listbox", { name: /^(Messages|Nachrichten) in/ });
  const row = within(listbox)
    .getAllByRole("option")
    .find((o) => optionName(o).includes(subject));
  if (!row) throw new Error("no row");
  await user.click(row);
  await screen.findByTestId("message-card");
}

const REQUEST = { to: "sender1@example.org", returnPathDiffers: false, answer: "ask" as const };

describe("read receipts: incoming requests", () => {
  it("shows a quiet question and sends nothing without a click", async () => {
    const { core, user, first } = setup({ receipt: { request: REQUEST } });
    await openMessage(user, first.subject);
    const bar = await screen.findByRole("group", { name: "The sender asks for a read receipt." });
    expect(bar.textContent).toContain("sender1@example.org learns that you opened this message");
    expect(within(bar).getByRole("button", { name: "Send receipt" })).toBeTruthy();
    expect(within(bar).getByRole("button", { name: "Don't send" })).toBeTruthy();
    // Asked only after the body is shown, for this message.
    expect(core.callsOf("get_read_receipt")).toEqual(expect.arrayContaining([{ messageId: first.id }]));
    // Opening and reading never answers.
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(core.callsOf("send_read_receipt")).toEqual([]);
    expect(core.callsOf("decline_read_receipt")).toEqual([]);
    expect(core.receipts.sentReceipts).toEqual([]);
    expect(core.callsOf("send_draft")).toEqual([]);
  });

  it("“Send receipt” sends it once, in the app's language, with a fresh request id", async () => {
    const { core, user, first } = setup({ receipt: { request: REQUEST } });
    await openMessage(user, first.subject);
    await user.click(await screen.findByRole("button", { name: "Send receipt" }));
    await waitFor(() => {
      expect(core.receipts.sentReceipts).toHaveLength(1);
    });
    const [call] = core.callsOf("send_read_receipt");
    expect(call?.["messageId"]).toBe(first.id);
    expect(call?.["language"]).toBe("en");
    expect(call?.["requestId"]).toMatch(UUID);
    const status = await screen.findByTestId("read-receipt-answered");
    expect(status.textContent).toBe("The read receipt is on its way.");
    expect(status.getAttribute("role")).toBe("status");
    expect(screen.queryByRole("button", { name: "Send receipt" })).toBeNull();
  });

  it("sends the German receipt text when the app is in German", async () => {
    const { core, user, first } = setup({ receipt: { request: REQUEST }, language: "de" });
    await openMessage(user, first.subject);
    await user.click(await screen.findByRole("button", { name: "Bestätigung senden" }));
    await waitFor(() => {
      expect(core.callsOf("send_read_receipt")[0]?.["language"]).toBe("de");
    });
  });

  it("“Don't send” records the answer and sends nothing", async () => {
    const { core, user, first } = setup({ receipt: { request: REQUEST } });
    await openMessage(user, first.subject);
    await user.click(await screen.findByRole("button", { name: "Don't send" }));
    await waitFor(() => {
      expect(core.receipts.declined).toEqual([first.id]);
    });
    const [decline] = core.callsOf("decline_read_receipt");
    expect(decline?.["messageId"]).toBe(first.id);
    expect(decline?.["requestId"]).toMatch(UUID);
    expect((await screen.findByTestId("read-receipt-answered")).textContent).toBe("No read receipt was sent.");
    expect(core.receipts.sentReceipts).toEqual([]);
    expect(core.callsOf("send_read_receipt")).toEqual([]);
  });

  it("warns when the receipt would go to another address than the return path", async () => {
    const { user, first } = setup({ receipt: { request: { ...REQUEST, to: "tracker@example.net", returnPathDiffers: true } } });
    await openMessage(user, first.subject);
    const warning = await screen.findByTestId("read-receipt-return-path");
    expect(warning.textContent).toContain("tracker@example.net");
  });

  it("offers nothing with the setting “Never send”, nor for answered or other answers", async () => {
    const { core, user, first } = setup({ receipt: { request: REQUEST }, settings: { sendDelaySeconds: 10, readReceipts: "never" } });
    await openMessage(user, first.subject);
    await waitFor(() => {
      expect(core.callsOf("get_read_receipt").length).toBeGreaterThan(0);
    });
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(screen.queryByTestId("read-receipt-request")).toBeNull();
    expect(screen.queryByTestId("read-receipt-answered")).toBeNull();
  });

  it("an answer from another device shows no question", async () => {
    const { core, user, first } = setup({ receipt: { request: { ...REQUEST, answer: "answered_elsewhere" } } });
    await openMessage(user, first.subject);
    await waitFor(() => {
      expect(core.callsOf("get_read_receipt").length).toBeGreaterThan(0);
    });
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(screen.queryByTestId("read-receipt-request")).toBeNull();
  });

  it("a read-only account says why the receipt was not sent and where to allow changes", async () => {
    const { core, user, first } = setup({ account: makeAccount({ writesEnabled: false }), receipt: { request: REQUEST } });
    await openMessage(user, first.subject);
    await user.click(await screen.findByRole("button", { name: "Send receipt" }));
    const error = await screen.findByTestId("read-receipt-send-error");
    expect(error.textContent).toContain("The read receipt was not sent.");
    expect(error.getAttribute("data-error-code")).toBe("read_only");
    expect(within(error).getByRole("button", { name: "Allow changes…" })).toBeTruthy();
    expect(core.receipts.sentReceipts).toEqual([]);
  });
});

describe("read receipts: own messages", () => {
  it("lists receipts that came back at the sent message", async () => {
    const receivedAt = new Date(2026, 10, 22, 9, 15).getTime();
    const { user, first } = setup({
      receipt: {
        requested: true,
        received: [
          { recipient: "bob@example.org", disposition: "displayed", receivedAt },
          { recipient: null, disposition: "deleted", receivedAt },
        ],
      },
    });
    await openMessage(user, first.subject);
    const section = await screen.findByRole("region", { name: "Read receipts" });
    const lines = within(section).getAllByTestId("read-receipt-received").map((li) => li.textContent);
    expect(lines[0]).toMatch(/^Displayed by bob@example\.org on .*2026/);
    expect(lines[1]).toMatch(/^Deleted without being read by a recipient on /);
  });

  it("says when a receipt was asked for and none came back yet", async () => {
    const { user, first } = setup({ receipt: { requested: true } });
    await openMessage(user, first.subject);
    expect((await screen.findByTestId("read-receipt-waiting")).textContent).toBe(
      "You asked for a read receipt. None has come back yet.",
    );
  });

  it("names a receipt as such", async () => {
    const { user, first } = setup({ receipt: { receiptFor: "<abc@example.org>" } });
    await openMessage(user, first.subject);
    expect((await screen.findByTestId("read-receipt-for")).textContent).toBe(
      "This message is a read receipt for one of your messages.",
    );
  });
});

describe("read receipts and send and archive: Settings → Writing", () => {
  it("stores “Never send” and “Archive the original” and keeps the other fields", async () => {
    const { core, user } = setup({ settings: { sendDelaySeconds: 5, defaultQuotePosition: "below" } });
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useUiStore.getState().showSettings("compose");
    });
    const policy = await screen.findByRole<HTMLSelectElement>("combobox", { name: "Read receipt requests" });
    await waitFor(() => {
      expect(policy.disabled).toBe(false);
    });
    expect(policy.value).toBe("ask");
    await user.selectOptions(policy, "never");
    await waitFor(() => {
      expect(core.callsOf("set_compose_settings").at(-1)).toEqual({
        settings: { sendDelaySeconds: 5, defaultQuotePosition: "below", readReceipts: "never" },
      });
    });
    await user.click(screen.getByRole("switch", { name: "Archive the original when I reply" }));
    await waitFor(() => {
      expect(core.callsOf("set_compose_settings").at(-1)).toEqual({
        settings: { sendDelaySeconds: 5, defaultQuotePosition: "below", readReceipts: "never", archiveOnReply: true },
      });
    });
    expect(screen.getByRole("switch", { name: "Archive the original when I reply" }).getAttribute("aria-checked")).toBe("true");
  });
});
