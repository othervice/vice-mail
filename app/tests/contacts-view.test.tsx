import { act, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";
import type { MessageListItem } from "../src/services/ipc";
import { useSearchStore } from "../src/stores/search";
import { useUiStore } from "../src/stores/ui";
import { makeContact } from "./support/contacts";
import { ipcError, makeAccount, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";

/*
 * Stage 17, block B (decision 0050): Contacts as a view of their own. Search, the detail with
 * note and recent mail, "Write mail", "Show all mail" and "Mark as spam" with the move to Junk,
 * read-only accounts and undo. Stage 22 (decision 0072): no pictures and no initials anywhere.
 */

const ADA = makeAccount({ writesEnabled: true });
const GRACE = makeContact({ email: "grace@example.org", name: "Grace Hopper", sent: 12, received: 30, lastUsed: Date.UTC(2026, 10, 20, 12) });
const ALAN = makeContact({ email: "alan@example.org", name: "Alan Turing", sent: 3, received: 1, lastUsed: Date.UTC(2026, 9, 2, 12) });
const PUSHY = makeContact({ email: "offers@spam.example", name: "Pushy", received: 4 });
const DAEMON = makeContact({ email: "mailer-daemon@example.org", name: "Mail Delivery System", received: 2, kind: "system" });

type User = ReturnType<typeof renderApp>["user"];

async function openView(user: User) {
  await user.click(await screen.findByRole("button", { name: "Contacts" }));
  const view = await screen.findByTestId("contacts-view");
  await within(view).findByRole("listbox", { name: "Contacts" });
  return view;
}

async function seriousFindings(): Promise<string[]> {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  const results = await axe.run(document.body, { resultTypes: ["violations"], iframes: false, rules: { "color-contrast": { enabled: false } } });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}

describe("Contacts view: no pictures (decision 0072)", () => {
  it("lists contacts by name and address, and the contact's page has no picture to choose", async () => {
    const core = mockCore({ accounts: [ADA], mailboxes: standardMailboxes(), contacts: { contacts: [GRACE, ALAN] } });
    const { user } = renderApp();
    const view = await openView(user);
    const rows = within(view).getAllByTestId("contact-row");
    expect(rows.map((row) => within(row).getByTestId("contact-name").textContent)).toEqual(["Grace Hopper", "Alan Turing"]);
    for (const row of rows) {
      expect(within(row).queryByTestId("avatar")).toBeNull();
      expect(row.querySelector("img")).toBeNull();
      // The name is the row's first content: nothing stands before it.
      expect(row.firstElementChild?.contains(within(row).getByTestId("contact-name"))).toBe(true);
    }
    const page = within(view).getByTestId("contact-detail");
    expect(within(page).queryByTestId("contact-picture-area")).toBeNull();
    expect(within(page).queryByRole("button", { name: /picture/i })).toBeNull();
    expect(page.querySelector("img")).toBeNull();
    expect(within(page).getByTestId("contact-detail-name").textContent).toBe("Grace Hopper");
    expect(core.callsOf("list_contact_pictures")).toEqual([]);
    expect(await seriousFindings()).toEqual([]);
  });

  it("shows no picture or initials in the message list, the reading pane or the composer's chips", async () => {
    // The list virtualizer needs a viewport height (jsdom has no layout).
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
    const message: MessageListItem = makeMessage({ subject: "Compilers", from: { name: "Grace Hopper", email: "Grace@Example.org" } });
    const core = mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: [message] },
      views: { [message.id]: makeView(message) },
      contacts: { contacts: [GRACE] },
    });
    const { user } = renderApp();
    const row = (await screen.findAllByRole("option"))[0] ?? document.body;
    expect(within(row).queryByTestId("avatar")).toBeNull();
    expect(within(row).getByTestId("row-sender").textContent).toBe("Grace Hopper");
    await user.click(row);
    const pane = await screen.findByRole("region", { name: "Message" });
    await within(pane).findByTestId("reading-from");
    expect(within(pane).queryByTestId("avatar")).toBeNull();
    expect(within(pane).getByTestId("reading-from").textContent).toContain("Grace Hopper");
    const view = await openView(user);
    await user.click(within(view).getByRole("button", { name: "Write mail" }));
    const chip = await screen.findByTestId("recipient-chip");
    expect(chip.textContent).toContain("Grace Hopper");
    expect(within(chip).queryByTestId("avatar")).toBeNull();
    // The core is never asked for pictures (the commands are gone).
    expect(core.callsOf("get_contact_picture")).toEqual([]);
    expect(core.callsOf("list_contact_pictures")).toEqual([]);
  });
});

describe("Contacts view: search, keyboard and the contact's page", () => {
  it("moves through the list with the arrow keys and goes to the contact with Enter", async () => {
    mockCore({ accounts: [ADA], contacts: { contacts: [GRACE, ALAN] } });
    const { user } = renderApp();
    const view = await openView(user);
    const search = within(view).getByLabelText("Search contacts");
    act(() => {
      search.focus();
    });
    await user.keyboard("{ArrowDown}");
    const list = within(view).getByRole("listbox", { name: "Contacts" });
    expect(document.activeElement).toBe(list);
    await user.keyboard("{ArrowDown}");
    expect(within(view).getByTestId("contact-detail-name").textContent).toBe("Alan Turing");
    expect(list.getAttribute("aria-activedescendant")).toBe(within(list).getAllByRole("option")[1]?.id);
    expect(within(list).getAllByRole("option").map((o) => o.getAttribute("aria-selected"))).toEqual(["false", "true"]);
    await user.keyboard("{Home}");
    expect(within(view).getByTestId("contact-detail-name").textContent).toBe("Grace Hopper");
    await user.keyboard("{Enter}");
    expect(document.activeElement).toBe(within(view).getByTestId("contact-detail-name"));
  });

  it("lists list and system addresses only on request, marked", async () => {
    const core = mockCore({ accounts: [ADA], contacts: { contacts: [GRACE, DAEMON] } });
    const { user } = renderApp();
    const view = await openView(user);
    expect(within(view).getAllByTestId("contact-name").map((n) => n.textContent)).toEqual(["Grace Hopper"]);
    await user.click(within(view).getByRole("checkbox", { name: "Show lists and system addresses" }));
    await waitFor(() => {
      expect(within(view).getAllByTestId("contact-row")).toHaveLength(2);
    });
    expect(core.callsOf("list_contacts").at(-1)?.["filter"]).toBe("all");
    const daemon = within(view).getAllByTestId("contact-row")[1];
    expect(within(daemon ?? view).getByTestId("contact-kind").textContent).toBe("System");
  });

  it("keeps a note, lists recent mail and opens it, writes mail and shows all mail from the contact", async () => {
    const recent = makeMessage({ id: 55, subject: "Re: compilers", accountId: ADA.id });
    const core = mockCore({ accounts: [ADA], mailboxes: standardMailboxes(), contacts: { contacts: [GRACE], messages: { "grace@example.org": [recent] } } });
    const { user } = renderApp();
    const view = await openView(user);
    const note = within(view).getByRole("textbox", { name: "Note" });
    await user.type(note, "Met at the fair");
    await user.click(within(view).getByRole("button", { name: "Save note" }));
    await waitFor(() => {
      expect(core.callsOf("set_contact_note")).toEqual([{ email: "grace@example.org", note: "Met at the fair" }]);
    });
    expect(await within(view).findByText("Saved.")).toBeTruthy();

    const mail = within(view).getByRole("list", { name: "Recent mail" });
    await user.click(within(mail).getByRole("button", { name: /Re: compilers/ }));
    await waitFor(() => {
      expect(core.callsOf("open_message_window")).toEqual([{ accountId: ADA.id, messageId: 55 }]);
    });

    await user.click(within(view).getByRole("button", { name: "Write mail" }));
    await waitFor(() => {
      expect(core.callsOf("create_draft")[0]?.["request"]).toMatchObject({
        kind: "new",
        accountId: ADA.id,
        to: [{ name: "Grace Hopper", email: "grace@example.org" }],
      });
    });

    await user.click(screen.getByRole("button", { name: "Show all mail" }));
    expect(useUiStore.getState().view).toEqual({ kind: "mail", mailView: "inbox" });
    expect(useSearchStore.getState()).toMatchObject({ open: true, query: "from:grace@example.org", submitted: true, scope: "all" });
  });
});

describe("Contacts view: mark as spam", () => {
  it("blocks, offers to move the existing mail to Junk, says what stayed, and undoes both", async () => {
    const core = mockCore({
      accounts: [ADA],
      contacts: {
        contacts: [PUSHY],
        spamMail: {
          "offers@spam.example": [
            { accountId: ADA.id, count: 3, writesEnabled: true },
            { accountId: makeAccount({ id: "00000000-0000-4000-8000-0000000000b2" }).id, count: 2, writesEnabled: false },
          ],
        },
      },
    });
    const { user } = renderApp();
    const view = await openView(user);
    await user.click(within(view).getByRole("button", { name: "Mark as spam" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Move 3 messages to Junk too?" });
    expect(dialog.textContent).toContain("Pushy is marked as spam: new mail from it goes to Junk.");
    expect(dialog.textContent).toContain("2 messages in read-only accounts stay where they are.");
    expect(await seriousFindings()).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "Move to Junk" }));
    const notice = await screen.findByText("Pushy is marked as spam; 3 messages moved to Junk.");
    expect(screen.getByText("2 messages in read-only accounts stayed where they are.")).toBeTruthy();
    expect(core.callsOf("move_contact_mail_to_junk")).toEqual([{ email: "offers@spam.example" }]);
    expect(core.contacts.blocked).toEqual(["offers@spam.example"]);
    await waitFor(() => {
      expect(within(view).getByTestId("contact-detail").textContent).toContain("Spam");
    });
    expect(within(view).getByRole("button", { name: "Not spam" })).toBeTruthy();

    const container = notice.closest('[data-testid="notice"]') ?? document.body;
    await user.click(within(container as HTMLElement).getByRole("button", { name: "Undo" }));
    await waitFor(() => {
      expect(core.contacts.undos).toEqual([{ addresses: ["offers@spam.example"], batches: [501] }]);
    });
    expect(core.contacts.blocked).toEqual([]);
    await waitFor(() => {
      expect(within(view).getByRole("button", { name: "Mark as spam" })).toBeTruthy();
    });
  });

  it("asks nothing when only read-only accounts have mail from it, and says it stays", async () => {
    const core = mockCore({
      accounts: [makeAccount({ writesEnabled: false })],
      contacts: { contacts: [PUSHY], spamMail: { "offers@spam.example": [{ accountId: ADA.id, count: 1, writesEnabled: false }] } },
    });
    const { user } = renderApp();
    const view = await openView(user);
    await user.click(within(view).getByRole("button", { name: "Mark as spam" }));
    expect(await screen.findByText("Pushy is marked as spam. New mail from it goes to Junk.")).toBeTruthy();
    expect(screen.getByText("1 message in a read-only account stayed where it is.")).toBeTruthy();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(core.callsOf("move_contact_mail_to_junk")).toEqual([]);
  });

  it("leaves the mail where it is on request; “Not spam” unblocks", async () => {
    const core = mockCore({
      accounts: [ADA],
      contacts: { contacts: [PUSHY], spamMail: { "offers@spam.example": [{ accountId: ADA.id, count: 4, writesEnabled: true }] } },
    });
    const { user } = renderApp();
    const view = await openView(user);
    await user.click(within(view).getByRole("button", { name: "Mark as spam" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Move 4 messages to Junk too?" });
    await user.click(within(dialog).getByRole("button", { name: "Leave it" }));
    expect(await screen.findByText("Pushy is marked as spam. New mail from it goes to Junk.")).toBeTruthy();
    expect(core.callsOf("move_contact_mail_to_junk")).toEqual([]);
    await user.click(await within(view).findByRole("button", { name: "Not spam" }));
    await waitFor(() => {
      expect(core.contacts.undos).toEqual([{ addresses: ["offers@spam.example"], batches: [] }]);
    });
    expect(await screen.findByText("Pushy is no longer marked as spam.")).toBeTruthy();
  });

  it("shows a refusal of the core in place", async () => {
    mockCore({ accounts: [ADA], contacts: { contacts: [PUSHY] }, failures: { mark_contact_spam: ipcError("database", "locked") } });
    const { user } = renderApp();
    const view = await openView(user);
    await user.click(within(view).getByRole("button", { name: "Mark as spam" }));
    const error = await within(view).findByTestId("contact-spam-error");
    expect(error.dataset["errorCode"]).toBe("database");
    expect(error.textContent).toContain("The contact could not be marked as spam.");
  });
});

describe("Contacts view in German", () => {
  it("speaks German (informal)", async () => {
    mockCore({
      accounts: [ADA],
      preferences: { theme: "light", language: "de" },
      contacts: { contacts: [PUSHY], spamMail: { "offers@spam.example": [{ accountId: ADA.id, count: 2, writesEnabled: true }] } },
    });
    const { user } = renderApp("de");
    await user.click(await screen.findByRole("button", { name: "Kontakte" }));
    const view = await screen.findByTestId("contacts-view");
    await within(view).findByRole("listbox", { name: "Kontakte" });
    expect(within(view).getByLabelText("Kontakte durchsuchen")).toBeTruthy();
    expect(within(view).getByRole("checkbox", { name: "Listen und Systemadressen zeigen" })).toBeTruthy();
    expect(within(view).getByRole("button", { name: "Mail schreiben" })).toBeTruthy();
    expect(within(view).getByRole("button", { name: "Alle Mails anzeigen" })).toBeTruthy();
    expect(within(view).queryByRole("button", { name: /Bild/ })).toBeNull();
    await user.click(within(view).getByRole("button", { name: "Als Spam markieren" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Auch 2 vorhandene Mails in den Spam-Ordner verschieben?" });
    await user.click(within(dialog).getByRole("button", { name: "In den Spam-Ordner" }));
    expect(await screen.findByText("Pushy ist als Spam markiert; 2 Mails sind im Spam-Ordner.")).toBeTruthy();
  });
});
