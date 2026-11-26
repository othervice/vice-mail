// Stage 23, item 2, step 3 (decision 0084): the toolbar across the window, the list's selection
// with and without the focus, the reading pane without its card (attachments as cards) and the
// contact panel on the right.
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ContactList, MessageListItem } from "../src/services/ipc";
import { useSearchStore } from "../src/stores/search";
import { useUiStore } from "../src/stores/ui";
import { makeContact } from "./support/contacts";
import { makeAccount, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";
import { DEFAULT_WINDOW_LAYOUT } from "./support/uiPrefs";

const ADA = makeAccount({ writesEnabled: true });
const now = Date.now();
const GRACE_MAIL: MessageListItem = makeMessage({
  id: 101,
  mailboxId: 1,
  subject: "Budget 2027",
  from: { name: "Grace Hopper", email: "grace@example.org" },
  date: now,
  hasAttachments: true,
});
const OLDER = makeMessage({ id: 100, mailboxId: 1, subject: "Earlier", from: { name: "Alan Turing", email: "alan@example.org" }, date: now - 3_600_000 });
const GRACE = makeContact({ email: "grace@example.org", name: "Grace Hopper", received: 3, note: "Ansprechpartnerin, Messe 2027" });
const GROUP: ContactList = {
  id: "0b5e8f43-6c1f-4a51-9a51-000000000001",
  name: "Messe 2027",
  members: [{ email: "grace@example.org", name: "Grace Hopper", contact: "grace@example.org" }],
};

const ATTACHMENTS = [
  { index: 0, name: "Angebot.pdf", mimeType: "application/pdf", size: 2_400_000, local: true, executable: false, preview: null },
  { index: 1, name: "Kalkulation.xlsx", mimeType: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", size: 86_000, local: true, executable: false, preview: null },
];

function setup({ panel = true, width = 1280 }: { panel?: boolean; width?: number } = {}) {
  vi.spyOn(window, "innerWidth", "get").mockReturnValue(width);
  // The list draws the rows it has room for.
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: [GRACE_MAIL, OLDER] },
    views: { [GRACE_MAIL.id]: makeView(GRACE_MAIL, { attachments: ATTACHMENTS }), [OLDER.id]: makeView(OLDER) },
    contacts: { contacts: [GRACE], messages: { "grace@example.org": [GRACE_MAIL] } },
    stage19: { lists: [GROUP] },
    ui: { layout: { ...DEFAULT_WINDOW_LAYOUT, contactPanel: panel } },
  });
  return { core, ...renderApp() };
}

async function openGrace(user: ReturnType<typeof renderApp>["user"]) {
  const list = await screen.findByRole("listbox", { name: /^Messages in/ });
  const row = (await within(list).findAllByRole("option")).find((option) => option.textContent?.includes("Budget 2027"));
  await user.click(row as HTMLElement);
  const pane = await screen.findByRole("region", { name: "Message" });
  await within(pane).findByTestId("reading-from");
  return { list, row: row as HTMLElement, pane };
}

async function seriousFindings(): Promise<string[]> {
  const results = await axe.run(document.body, { resultTypes: ["violations"], iframes: false, rules: { "color-contrast": { enabled: false } } });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("the toolbar across the window", () => {
  it("holds the sidebar toggle, New mail, the view's head and the open message's actions; the pane has no bar of its own", async () => {
    const { user } = setup({ panel: false });
    const { pane } = await openGrace(user);
    // The toolbar is the window's banner (testing-library counts the message's head as one too).
    const banner = screen.getByTestId("app-toolbar");
    expect(banner.tagName).toBe("HEADER");
    expect(within(banner).getByRole("button", { name: "Show sidebar" }).getAttribute("aria-expanded")).toBe("false");
    expect(within(banner).getByRole("button", { name: "New message" })).toBeTruthy();
    expect(within(banner).getByRole("heading", { level: 1, name: "Inbox" })).toBeTruthy();
    expect(within(banner).getByTestId("view-unread").textContent).toBe("3 unread");
    expect(within(banner).getByRole("button", { name: "Quick filter" })).toBeTruthy();
    expect(within(banner).getByRole("button", { name: "Search" })).toBeTruthy();
    // The message's actions are in the toolbar's segment over the reading pane.
    const actions = within(banner).getByRole("toolbar", { name: "Message actions" });
    expect(within(screen.getByTestId("toolbar-reading")).getByRole("toolbar", { name: "Message actions" })).toBe(actions);
    expect(within(pane).queryByRole("toolbar")).toBeNull();
    // The pane keeps its quiet tools and its close button in its top line, with account and folder.
    const meta = within(pane).getByTestId("reading-meta");
    expect(meta.textContent).toContain("Ada Lovelace · Inbox");
    expect(within(meta).getByRole("button", { name: "Close message" })).toBeTruthy();
    expect(within(meta).getByRole("group", { name: "Export and print" })).toBeTruthy();
  });

  it("names the toggle for what it does and keeps it while the column folds and unfolds", async () => {
    const { user } = setup({ panel: false });
    await openGrace(user);
    const toggle = screen.getByRole("button", { name: "Show sidebar" });
    await user.click(toggle);
    await screen.findByTestId("sidebar-column");
    expect(toggle.isConnected).toBe(true);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(toggle.getAttribute("aria-label")).toBe("Hide sidebar");
  });
});

describe("the list's selection", () => {
  it("is filled while the list has the focus and neutral when the focus is elsewhere", async () => {
    const { user } = setup({ panel: false });
    const { list, row, pane } = await openGrace(user);
    const drawn = row.querySelector("[data-row-selected]");
    expect(drawn).not.toBeNull();
    expect(drawn?.className).toContain("bg-selected-muted");
    expect(list.hasAttribute("data-list-focus")).toBe(true);
    // The click focused the list: its selection shows filled (app.css, `[data-list-active]`).
    expect(document.activeElement).toBe(list);
    expect(list.hasAttribute("data-list-active")).toBe(true);
    act(() => {
      within(pane).getByRole("article").focus();
    });
    expect(list.hasAttribute("data-list-active")).toBe(false);
    act(() => {
      list.focus();
    });
    expect(list.hasAttribute("data-list-active")).toBe(true);
  });

  it("draws a hairline under every row and no account stripe where all rows share one account", async () => {
    const { user } = setup({ panel: false });
    const { list } = await openGrace(user);
    for (const option of within(list).getAllByRole("option")) {
      expect(option.querySelector(".border-b")).not.toBeNull();
      expect(within(option).queryByTestId("row-account")).toBeNull();
    }
  });
});

describe("the reading pane without its card", () => {
  it("shows the attachments below the text as cards tinted by their kind, with their count and Save all", async () => {
    const { user } = setup({ panel: false });
    const { pane } = await openGrace(user);
    const section = await within(pane).findByRole("region", { name: "2 attachments" });
    expect(section.textContent).toContain("2 attachments");
    expect(within(section).getByTestId("attachments-save-all")).toBeTruthy();
    const badges = within(section).getAllByTestId("attachment-badge");
    expect(badges.map((badge) => badge.textContent)).toEqual(["PDF", "XLS"]);
    expect(badges.map((badge) => badge.dataset["tint"])).toEqual(["pdf", "sheet"]);
    const cards = within(section).getAllByTestId("attachment");
    expect(cards[0]?.className).toContain("bg-file-pdf-tint");
    expect(cards[1]?.className).toContain("bg-file-sheet-tint");
    // Their buttons stay in the Tab order (they show on hover and with the focus in the card).
    expect(within(cards[0] as HTMLElement).getByRole("button", { name: "Save Angebot.pdf" })).toBeTruthy();
    // Below the text: the body comes first in the message.
    const card = within(pane).getByTestId("message-card");
    const frame = within(card).getByTestId("mail-frame");
    expect(frame.compareDocumentPosition(section) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});

describe("the contact panel", () => {
  it("shows the sender from the contacts: name, address, note, recent mail, groups", async () => {
    const { user } = setup();
    await openGrace(user);
    const panel = await screen.findByRole("complementary", { name: "Contact" });
    expect(await within(panel).findByRole("heading", { level: 2, name: "Grace Hopper" })).toBeTruthy();
    expect(within(panel).getByTestId("contact-panel-email").textContent).toBe("grace@example.org");
    expect((await within(panel).findByTestId("contact-panel-note")).textContent).toContain("Ansprechpartnerin, Messe 2027");
    const mails = await within(panel).findByRole("list", { name: "Mail" });
    expect(within(mails).getAllByRole("button").map((b) => b.textContent)).toEqual([expect.stringContaining("Budget 2027")]);
    const groups = await within(panel).findByTestId("contact-panel-groups");
    expect(groups.textContent).toContain("Messe 2027");
    expect(await seriousFindings()).toEqual([]);
  });

  it("writes to the person from the message's account and finds all mail with the contact search", async () => {
    const { core, user } = setup();
    await openGrace(user);
    const panel = await screen.findByRole("complementary", { name: "Contact" });
    await within(panel).findByRole("heading", { level: 2, name: "Grace Hopper" });
    await user.click(within(panel).getByRole("button", { name: "Write mail" }));
    await waitFor(() => {
      expect(core.callsOf("create_draft")[0]?.["request"]).toMatchObject({
        kind: "new",
        accountId: ADA.id,
        to: [{ name: "Grace Hopper", email: "grace@example.org" }],
      });
    });
    // The composer card takes the panel's room while it is open (decision 0086, addendum).
    await waitFor(() => {
      expect(screen.queryByRole("complementary", { name: "Contact" })).toBeNull();
    });
    await user.click(await screen.findByTestId("composer-close"));
    const back = await screen.findByRole("complementary", { name: "Contact" });
    await user.click(within(back).getByRole("button", { name: "All mail" }));
    expect(useUiStore.getState().view).toEqual({ kind: "mail", mailView: "inbox" });
    expect(useSearchStore.getState()).toMatchObject({ open: true, query: "contact:grace@example.org", submitted: true, scope: "all" });
  });

  it("is shown and hidden from the toolbar and stored with the window's layout", async () => {
    const { core, user } = setup();
    await openGrace(user);
    const toggle = screen.getByRole("button", { name: "Contact panel" });
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(toggle.getAttribute("aria-controls")).toBe(screen.getByRole("complementary", { name: "Contact" }).id);
    await user.click(toggle);
    expect(screen.queryByRole("complementary", { name: "Contact" })).toBeNull();
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    await waitFor(() => {
      expect(core.ui.layout.contactPanel).toBe(false);
    });
  });

  it("names a sender who is no contact by the message and has no room in a narrow window", async () => {
    const { user } = setup();
    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    const older = (await within(list).findAllByRole("option")).find((option) => option.textContent?.includes("Earlier"));
    await user.click(older as HTMLElement);
    const panel = await screen.findByRole("complementary", { name: "Contact" });
    expect(await within(panel).findByRole("heading", { level: 2, name: "Alan Turing" })).toBeTruthy();
    expect(within(panel).queryByRole("alert")).toBeNull();
    expect(within(panel).queryByTestId("contact-panel-note")).toBeNull();
    expect(within(panel).queryByTestId("contact-panel-mails")).toBeNull();

    vi.spyOn(window, "innerWidth", "get").mockReturnValue(900);
    act(() => {
      fireEvent(window, new Event("resize"));
    });
    expect(screen.queryByRole("complementary", { name: "Contact" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Contact panel" })).toBeNull();
  });

  // Stage 23 integration (decision 0086, addendum): the composer card takes the reading area and
  // the panel's room, as in the draft; minimised or closed, the panel is back.
  it("steps aside while the composer card covers the reading area", async () => {
    const { user } = setup();
    await openGrace(user);
    expect(await screen.findByRole("complementary", { name: "Contact" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "New message" }));
    await screen.findByRole("dialog", { name: "New message" });
    await waitFor(() => {
      expect(screen.queryByRole("complementary", { name: "Contact" })).toBeNull();
    });
    expect(screen.queryByRole("button", { name: "Contact panel" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Minimise" }));
    expect(await screen.findByRole("complementary", { name: "Contact" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Contact panel" }).getAttribute("aria-expanded")).toBe("true");
    await user.click(screen.getByTestId("composer-restore"));
    await waitFor(() => {
      expect(screen.queryByRole("complementary", { name: "Contact" })).toBeNull();
    });
    await user.click(screen.getByTestId("composer-close"));
    expect(await screen.findByRole("complementary", { name: "Contact" })).toBeTruthy();
  });
});
