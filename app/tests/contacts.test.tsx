import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ContactDuplicate, ContactEntry } from "../src/services/ipc";
import { makeContact } from "./support/contacts";
import { ipcError, makeAccount, mockCore } from "./support/core";
import { renderApp } from "./support/render";

const GRACE = makeContact({ email: "grace@example.org", name: "Grace Hopper", sent: 12, received: 30, lastUsed: Date.UTC(2026, 10, 20, 12) });
const ALAN = makeContact({ email: "alan@example.org", name: "Alan Turing", sent: 3, received: 1, lastUsed: Date.UTC(2026, 9, 2, 12) });
const NONAME = makeContact({ email: "x7@example.net", received: 1 });
const LINUS = makeContact({ email: "linus@example.org", name: "Linus", sent: 2 });
const LINUS_LISTS = makeContact({ email: "linus+lists@example.org", name: "Linus", received: 5 });
const KIM = makeContact({ email: "kim@example.org", name: "Kim Lee", received: 4 });

const PLUS: ContactDuplicate = { reason: "plus_address", name: null, names: [], contacts: [LINUS, LINUS_LISTS] };
const KIM_NAMES: ContactDuplicate = { reason: "names_differ", name: null, names: ["Kim Lee", "K. Lee"], contacts: [KIM] };

/** A contact merged from two addresses earlier. */
const MERGED: ContactEntry = {
  ...makeContact({ email: "ada@example.org", name: "Ada", sent: 5, received: 2 }),
  addresses: [
    { email: "ada@example.org", sent: 4, received: 1, seen: 0, lastUsed: null, blocked: false },
    { email: "ada@old.example.org", sent: 1, received: 1, seen: 0, lastUsed: null, blocked: false },
  ],
};

/** Settings → Contacts: the link to the view and the duplicate suggestions. */
async function openContacts(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await user.click(screen.getByRole("tab", { name: "Contacts" }));
  return screen.getByRole("tabpanel", { name: "Contacts" });
}

/** Stage 17 (decision 0050): the contacts view, from the rail. */
async function openView(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("button", { name: "Contacts" }));
  return screen.findByTestId("contacts-view");
}

const names = (panel: HTMLElement) => within(panel).getAllByTestId("contact-name").map((n) => n.textContent);

describe("Contacts view: the directory", () => {
  it("lists the contacts in the core's ranking with the date of the last mail; the first is open", async () => {
    mockCore({ accounts: [makeAccount()], contacts: { contacts: [GRACE, ALAN, NONAME] } });
    const { user } = renderApp();
    const view = await openView(user);
    const list = await within(view).findByRole("listbox", { name: "Contacts" });
    expect(names(list)).toEqual(["Grace Hopper", "Alan Turing", "(no name)"]);
    const grace = within(list).getAllByTestId("contact-row")[0];
    expect(grace?.textContent).toContain("grace@example.org");
    expect(within(grace ?? list).getByTestId("contact-last-used").textContent).toBe("Nov 20");
    const detail = within(view).getByTestId("contact-detail");
    expect(within(detail).getByTestId("contact-detail-name").textContent).toBe("Grace Hopper");
    expect(within(detail).getByTestId("contact-usage").textContent).toMatch(/^12 sent · 30 received · last Nov 20, 2026$/);
  });

  it("says where contacts come from when there are none yet, without examples", async () => {
    mockCore({ accounts: [makeAccount()] });
    const { user } = renderApp();
    const view = await openView(user);
    expect((await within(view).findByTestId("contacts-empty")).textContent).toContain("People appear here once you write to them");
    expect(within(view).getByText("No contacts yet", { selector: "h2" })).toBeTruthy();
    expect(within(view).queryByTestId("contact-row")).toBeNull();
  });

  it("searches name, address and domain as the user types", async () => {
    const core = mockCore({ accounts: [makeAccount()], contacts: { contacts: [GRACE, ALAN] } });
    const { user } = renderApp();
    const view = await openView(user);
    await within(view).findByRole("listbox", { name: "Contacts" });
    await user.type(within(view).getByLabelText("Search contacts"), "alan");
    await waitFor(() => {
      expect(names(view)).toEqual(["Alan Turing"]);
    });
    expect(core.callsOf("list_contacts").at(-1)).toEqual({ query: "alan", cursor: null, limit: 50, filter: "people" });
    await user.clear(within(view).getByLabelText("Search contacts"));
    await user.type(within(view).getByLabelText("Search contacts"), "nobody");
    expect((await within(view).findByTestId("contacts-no-match")).textContent).toBe("No contact matches “nobody”.");
  });

  it("loads the next page with the core's cursor", async () => {
    const many = Array.from({ length: 55 }, (_, i) => makeContact({ email: `person${String(i)}@example.org`, name: `Person ${String(i)}` }));
    const core = mockCore({ accounts: [makeAccount()], contacts: { contacts: many } });
    const { user } = renderApp();
    const view = await openView(user);
    await within(view).findByRole("listbox", { name: "Contacts" });
    expect(within(view).getAllByTestId("contact-row")).toHaveLength(50);
    await user.click(within(view).getByRole("button", { name: "Show more contacts" }));
    await waitFor(() => {
      expect(within(view).getAllByTestId("contact-row")).toHaveLength(55);
    });
    expect(core.callsOf("list_contacts").at(-1)).toEqual({ query: null, cursor: "50", limit: 50, filter: "people" });
    expect(within(view).queryByRole("button", { name: "Show more contacts" })).toBeNull();
  });

  it("names the rename button with its visible text first (WCAG 2.5.3, label in name)", async () => {
    mockCore({ accounts: [makeAccount()], contacts: { contacts: [GRACE, ALAN] } });
    const { user } = renderApp();
    const view = await openView(user);
    await within(view).findByRole("listbox", { name: "Contacts" });
    const button = within(view).getByRole("button", { name: "Edit name of grace@example.org" });
    const visible = button.textContent?.trim() ?? "";
    expect(visible).toBe("Edit name");
    expect(button.getAttribute("aria-label")).toContain(visible);
  });

  it("changes a name and goes back to the name seen in mail", async () => {
    const core = mockCore({ accounts: [makeAccount()], contacts: { contacts: [GRACE, ALAN] } });
    const { user } = renderApp();
    const view = await openView(user);
    const list = await within(view).findByRole("listbox", { name: "Contacts" });
    await user.click(within(view).getByRole("button", { name: "Edit name of grace@example.org" }));
    const field = within(view).getByLabelText("Name");
    expect((field as HTMLInputElement).value).toBe("Grace Hopper");
    await user.clear(field);
    await user.type(field, "Amazing Grace{Enter}");
    await waitFor(() => {
      expect(names(list)[0]).toBe("Amazing Grace");
    });
    expect(core.callsOf("set_contact_name")).toEqual([{ email: "grace@example.org", name: "Amazing Grace" }]);
    expect(within(view).getByTestId("contact-detail").textContent).toContain("You gave this name.");

    await user.click(within(view).getByRole("button", { name: "Edit name of grace@example.org" }));
    await user.click(within(view).getByRole("button", { name: "Use the name from mail" }));
    await waitFor(() => {
      expect(names(list)[0]).toBe("Grace Hopper");
    });
    expect(core.callsOf("set_contact_name").at(-1)).toEqual({ email: "grace@example.org", name: null });
  });

  it("cancels editing a name with Escape and shows a refused name in place", async () => {
    const core = mockCore({
      accounts: [makeAccount()],
      contacts: { contacts: [GRACE] },
      failures: { set_contact_name: ipcError("invalid_argument", "name too long") },
    });
    const { user } = renderApp();
    const view = await openView(user);
    await within(view).findByRole("listbox", { name: "Contacts" });
    await user.click(within(view).getByRole("button", { name: "Edit name of grace@example.org" }));
    await user.keyboard("{Escape}");
    expect(within(view).queryByLabelText("Name")).toBeNull();

    await user.click(within(view).getByRole("button", { name: "Edit name of grace@example.org" }));
    await user.click(within(view).getByRole("button", { name: "Save name" }));
    const error = await within(view).findByTestId("contact-name-error");
    expect(error.dataset["errorCode"]).toBe("invalid_argument");
    expect(error.textContent).toContain("The name could not be changed.");
    expect(core.contacts.contacts[0]?.name).toBe("Grace Hopper");
  });

  it("separates an address of a merged contact", async () => {
    const core = mockCore({ accounts: [makeAccount()], contacts: { contacts: [MERGED, GRACE] } });
    const { user } = renderApp();
    const view = await openView(user);
    await within(view).findByRole("listbox", { name: "Contacts" });
    const addresses = within(view).getByRole("list", { name: "Addresses of Ada" });
    expect(within(addresses).getAllByTestId("contact-address")).toHaveLength(2);
    await user.click(within(view).getByRole("button", { name: "Separate ada@old.example.org into a contact of its own" }));
    await waitFor(() => {
      expect(within(view).getAllByTestId("contact-email").map((e) => e.textContent)).toEqual([
        "ada@example.org",
        "ada@old.example.org",
        "grace@example.org",
      ]);
    });
    expect(core.callsOf("separate_contact")).toEqual([{ email: "ada@old.example.org" }]);
    await waitFor(() => {
      expect(within(within(view).getByRole("list", { name: "Addresses of Ada" })).getAllByTestId("contact-address")).toHaveLength(1);
    });
  });

  it("shows a failed load with a way to try again", async () => {
    let failing = true;
    mockCore({
      accounts: [makeAccount()],
      contacts: { contacts: [GRACE] },
      fail: (cmd) => (cmd === "list_contacts" && failing ? ipcError("internal", "database locked") : undefined),
    });
    const { user } = renderApp();
    const view = await openView(user);
    const error = await within(view).findByTestId("contacts-error");
    expect(error.textContent).toContain("The contacts could not be loaded.");
    failing = false;
    await user.click(within(error).getByRole("button", { name: "Try again" }));
    await within(view).findByRole("listbox", { name: "Contacts" });
    expect(names(view)).toEqual(["Grace Hopper"]);
  });
});

describe("Settings, contacts", () => {
  it("links to the contacts view", async () => {
    mockCore({ accounts: [makeAccount()], contacts: { contacts: [GRACE] } });
    const { user } = renderApp();
    const panel = await openContacts(user);
    expect(panel.textContent).toContain("Built on this device from every address");
    await user.click(within(panel).getByRole("button", { name: "Open contacts" }));
    expect(await screen.findByTestId("contacts-view")).toBeTruthy();
  });
});

describe("Settings, contacts: duplicates", () => {
  it("lists the suggestions with their reason", async () => {
    mockCore({ accounts: [makeAccount()], contacts: { contacts: [LINUS, LINUS_LISTS, KIM], duplicates: [PLUS, KIM_NAMES] } });
    const { user } = renderApp();
    const panel = await openContacts(user);
    const section = await within(panel).findByRole("region", { name: "2 possible duplicates" });
    const cards = within(section).getAllByTestId("contact-duplicate");
    expect(cards.map((c) => within(c).getByTestId("contact-duplicate-reason").textContent)).toEqual([
      "One address is the other with a “+” tag.",
      "This address was seen with different names.",
    ]);
    expect(section.textContent).toContain("Nothing is merged unless you confirm it.");
    expect(cards[1]?.textContent).toContain("kim@example.org appears as: Kim Lee, K. Lee");
  });

  it("merges only after the user confirmed in the dialog", async () => {
    const core = mockCore({ accounts: [makeAccount()], contacts: { contacts: [LINUS, LINUS_LISTS, GRACE], duplicates: [PLUS] } });
    const { user } = renderApp();
    const panel = await openContacts(user);
    const card = await within(panel).findByTestId("contact-duplicate");
    const keep = within(card).getByRole("group", { name: "Address to keep as the main one" });
    expect(within(keep).getAllByRole("radio").map((r) => (r as HTMLInputElement).checked)).toEqual([true, false]);

    await user.click(within(card).getByRole("button", { name: "Merge…" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Merge these contacts?" });
    expect(dialog.textContent).toContain("linus+lists@example.org becomes part of the contact linus@example.org.");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(core.callsOf("merge_contacts")).toEqual([]);

    await user.click(within(keep).getByRole("radio", { name: "Linus <linus+lists@example.org>" }));
    await user.click(within(card).getByRole("button", { name: "Merge…" }));
    const again = await screen.findByRole("alertdialog", { name: "Merge these contacts?" });
    expect(again.textContent).toContain("linus@example.org becomes part of the contact linus+lists@example.org.");
    await user.click(within(again).getByRole("button", { name: "Merge" }));

    await waitFor(() => {
      expect(within(panel).queryByTestId("contact-duplicates")).toBeNull();
    });
    expect(core.callsOf("merge_contacts")).toEqual([
      { request: { keep: "linus+lists@example.org", merge: ["linus@example.org"], confirmed: true } },
    ]);
    expect(core.contacts.contacts.map((c) => c.addresses.map((a) => a.email))).toEqual([
      ["linus+lists@example.org", "linus@example.org"],
      ["grace@example.org"],
    ]);
  });

  it("shows the core's refusal in place and changes nothing", async () => {
    const core = mockCore({
      accounts: [makeAccount()],
      contacts: { contacts: [LINUS, LINUS_LISTS], duplicates: [PLUS] },
      failures: { merge_contacts: ipcError("confirmation_required") },
    });
    const { user } = renderApp();
    const panel = await openContacts(user);
    const card = await within(panel).findByTestId("contact-duplicate");
    await user.click(within(card).getByRole("button", { name: "Merge…" }));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Merge" }));
    const error = await within(card).findByTestId("contact-duplicate-error");
    expect(error.dataset["errorCode"]).toBe("confirmation_required");
    expect(error.textContent).toContain("The suggestion could not be handled.");
    expect(core.contacts.merges).toEqual([]);
    expect(core.contacts.contacts).toHaveLength(2);
  });

  it("dismisses a suggestion for good", async () => {
    const core = mockCore({ accounts: [makeAccount()], contacts: { contacts: [LINUS, LINUS_LISTS], duplicates: [PLUS] } });
    const { user } = renderApp();
    const panel = await openContacts(user);
    const card = await within(panel).findByTestId("contact-duplicate");
    await user.click(within(card).getByRole("button", { name: "Not the same person" }));
    await waitFor(() => {
      expect(within(panel).queryByTestId("contact-duplicates")).toBeNull();
    });
    expect(core.callsOf("dismiss_contact_duplicate")).toEqual([{ contacts: ["linus@example.org", "linus+lists@example.org"] }]);
    expect(core.callsOf("merge_contacts")).toEqual([]);
  });

  it("picks one of the names a contact was seen with", async () => {
    const core = mockCore({ accounts: [makeAccount()], contacts: { contacts: [KIM], duplicates: [KIM_NAMES] } });
    const { user } = renderApp();
    const panel = await openContacts(user);
    const card = await within(panel).findByTestId("contact-duplicate");
    await user.click(within(card).getByRole("button", { name: "Use “K. Lee”" }));
    await waitFor(() => {
      expect(core.contacts.contacts[0]?.name).toBe("K. Lee");
    });
    expect(core.callsOf("set_contact_name")).toEqual([{ email: "kim@example.org", name: "K. Lee" }]);
    // Leaving it as it is hides the question on this device.
    await user.click(within(card).getByRole("button", { name: "Leave it as it is" }));
    await waitFor(() => {
      expect(within(panel).queryByTestId("contact-duplicates")).toBeNull();
    });
    expect(core.callsOf("dismiss_contact_duplicate")).toEqual([{ contacts: ["kim@example.org"] }]);
  });

  it("speaks German", async () => {
    mockCore({
      accounts: [makeAccount()],
      preferences: { theme: "light", language: "de" },
      contacts: { contacts: [LINUS, LINUS_LISTS], duplicates: [PLUS] },
    });
    const { user } = renderApp("de");
    await user.click(await screen.findByRole("button", { name: "Einstellungen" }));
    await user.click(screen.getByRole("tab", { name: "Kontakte" }));
    const panel = screen.getByRole("tabpanel", { name: "Kontakte" });
    const card = await within(panel).findByTestId("contact-duplicate");
    expect(card.textContent).toContain("Eine Adresse ist die andere mit einem „+“-Zusatz.");
    expect(within(card).getByRole("button", { name: "Zusammenführen …" })).toBeTruthy();
    expect(within(panel).getByRole("button", { name: "Kontakte öffnen" })).toBeTruthy();
  });
});
