// Stage 19 (decisions 0056, 0057): contact groups on the contacts page and in the composer, vCard
// import and export, mbox export and import of folders, and the takeover from Thunderbird,
// through the real app against the fake core; with the axe checks of these views.
import { act, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ContactList, MboxJob, TakeoverPreview } from "../src/services/ipc";
import { openComposer } from "../src/stores/composer";
import { useUiStore } from "../src/stores/ui";
import { makeContact } from "./support/contacts";
import { makeAccount, makeMessages, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

const ADA = makeAccount({ writesEnabled: true });
type User = ReturnType<typeof renderApp>["user"];

async function seriousFindings(): Promise<string[]> {
  const results = await axe.run(document.body, { resultTypes: ["violations"], iframes: false, rules: { "color-contrast": { enabled: false } } });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}

function group(name: string, emails: string[], n = 1): ContactList {
  return {
    id: `0b5e8f43-6c1f-4a51-9a51-${n.toString(16).padStart(12, "a")}`,
    name,
    members: emails.map((email) => ({ email, name: null, contact: null })),
  };
}

async function openFolderMenu(user: User, name: string) {
  if (!screen.queryByTestId("sidebar-column")) {
    await user.click(await screen.findByRole("button", { name: "Folders" }));
    await screen.findByTestId("sidebar-column");
  }
  await user.click(await screen.findByRole("button", { name: `Options for ${name}` }));
  return screen.findByRole("menu");
}

async function openGroups(user: User) {
  await user.click(await screen.findByRole("button", { name: "Contacts" }));
  const view = await screen.findByTestId("contacts-view");
  await user.click(within(view).getByRole("tab", { name: "Groups" }));
  return view;
}

describe("contact groups on the contacts page", () => {
  it("creates a group, adds and removes members, renames and deletes it", async () => {
    const core = mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      contacts: { contacts: [makeContact({ email: "grace@example.org", name: "Grace Hopper", sent: 3 })] },
    });
    const { user } = renderApp();
    const view = await openGroups(user);
    expect(await within(view).findByTestId("contact-groups-empty")).toBeTruthy();
    await user.click(within(view).getByTestId("contact-group-new"));
    await user.type(within(view).getByTestId("contact-group-name-input"), "Choir");
    await user.click(within(view).getByTestId("contact-group-create"));
    const detail = await within(view).findByTestId("contact-group-detail");
    expect(within(detail).getByTestId("contact-group-title").textContent).toBe("Choir");
    expect(await seriousFindings()).toEqual([]);

    const field = within(within(detail).getByTestId("contact-group-add-field")).getByRole("combobox");
    await user.type(field, "ada@example.org, linus@example.org,");
    await user.click(within(detail).getByTestId("contact-group-add-submit"));
    await waitFor(() => {
      expect(within(detail).getAllByTestId("contact-group-member")).toHaveLength(2);
    });
    expect(core.stage19.lists[0]?.members.map((m) => m.email)).toEqual(["ada@example.org", "linus@example.org"]);
    await user.click(within(detail).getByRole("button", { name: "Remove ada@example.org from the group" }));
    await waitFor(() => {
      expect(within(detail).getAllByTestId("contact-group-member")).toHaveLength(1);
    });

    await user.click(within(detail).getByRole("button", { name: "Rename group" }));
    const rename = within(detail).getByTestId("contact-group-rename-input");
    await user.clear(rename);
    await user.type(rename, "Choir 2026");
    await user.click(within(detail).getByTestId("contact-group-rename-save"));
    await waitFor(() => {
      expect(within(view).getByTestId("contact-group-title").textContent).toBe("Choir 2026");
    });

    await user.click(within(view).getByTestId("contact-group-delete"));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog.textContent).toContain("Its members stay in your contacts");
    await user.click(within(dialog).getByRole("button", { name: "Delete group" }));
    await waitFor(() => {
      expect(core.stage19.lists).toEqual([]);
    });
    expect(await within(view).findByTestId("contact-groups-empty")).toBeTruthy();
  });
});

describe("groups in the composer", () => {
  it("suggests a group, expands it into its members and offers Bcc for many", async () => {
    const many = Array.from({ length: 11 }, (_, n) => `member${String(n)}@example.org`);
    mockCore({ accounts: [ADA], mailboxes: standardMailboxes(), stage19: { lists: [group("Newsletter readers", many)] } });
    const { user } = renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    act(() => {
      openComposer({ kind: "new", accountId: ADA.id });
    });
    const to = await screen.findByTestId("composer-to");
    await user.type(within(to).getByRole("combobox"), "newsl");
    const suggestion = await screen.findByTestId("recipient-group-suggestion");
    expect(suggestion.textContent).toContain("Newsletter readers");
    expect(suggestion.textContent).toContain("Group · 11 members");
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(within(to).getAllByTestId("recipient-chip")).toHaveLength(11);
    });
    const hint = await screen.findByTestId("composer-bcc-hint");
    expect(hint.textContent).toContain("11 recipients will see each other’s addresses");
    expect(await seriousFindings()).toEqual([]);
    await user.click(within(hint).getByTestId("composer-move-to-bcc"));
    await waitFor(() => {
      expect(within(screen.getByTestId("composer-bcc")).getAllByTestId("recipient-chip")).toHaveLength(11);
    });
    expect(within(to).queryAllByTestId("recipient-chip")).toHaveLength(0);
    expect(screen.queryByTestId("composer-bcc-hint")).toBeNull();
  });
});

describe("vCard import and export", () => {
  it("imports a file and reports what it did; exports all contacts", async () => {
    const core = mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      contacts: { contacts: [makeContact({ email: "grace@example.org", name: "Grace Hopper", sent: 3 })] },
      stage19: {
        vcardImport: {
          kind: "imported",
          report: { cards: 3, created: 2, updated: 1, merged: 1, lists: 1, problems: [{ card: 2, kind: "no_address" }] },
        },
      },
    });
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Contacts" }));
    await screen.findByTestId("contacts-view");
    await user.click(screen.getByTestId("contacts-import"));
    const report = await screen.findByTestId("contacts-import-report");
    expect(report.textContent).toContain("3 cards read");
    expect(report.textContent).toContain("New contacts: 2, updated: 1, groups: 1.");
    expect(report.textContent).toContain("1 card or line could not be taken over completely.");
    await user.click(screen.getByRole("button", { name: "Export as vCard" }));
    await user.click(await screen.findByRole("menuitem", { name: "Export all contacts (vCard 4.0)…" }));
    expect((await screen.findByTestId("contacts-export-saved")).textContent).toContain("/Users/test/Downloads/contacts.vcf");
    expect(core.callsOf("export_vcards")).toEqual([{ selection: { kind: "all", peopleOnly: true }, version: "v4" }]);
  });
});

describe("mbox export and import", () => {
  it("exports a folder from its menu, shows the progress and cancels", async () => {
    const items = makeMessages(3);
    const core = mockCore({ accounts: [ADA], mailboxes: standardMailboxes(), messages: { 1: items } });
    const { user } = renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    await openFolderMenu(user, "Inbox");
    await user.click(await screen.findByRole("menuitem", { name: "Export as mbox…" }));
    const jobNotice = await screen.findByTestId("mbox-job");
    expect(jobNotice.textContent).toContain("Exporting “INBOX”");
    const started = core.stage19.mboxJobs[0] as MboxJob;
    await emitCore({ type: "mboxJobChanged", job: { ...started, done: 2 } });
    await waitFor(() => {
      expect(screen.getByTestId("mbox-job-progress").textContent).toContain("2 of 3 messages written.");
    });
    expect(await seriousFindings()).toEqual([]);
    await user.click(screen.getByTestId("mbox-job-cancel"));
    await waitFor(() => {
      expect(core.stage19.mboxCancelled).toEqual([started.id]);
    });
    await emitCore({ type: "mboxJobChanged", job: { ...started, state: "cancelled" } });
    await waitFor(() => {
      expect(screen.getByTestId("mbox-job").textContent).toContain("Export of “INBOX” cancelled");
    });
  });

  it("keeps the import in the menu of a read-only account and says why", async () => {
    const readOnly = makeAccount({ writesEnabled: false });
    const core = mockCore({ accounts: [readOnly], mailboxes: standardMailboxes(), messages: { 1: makeMessages(1) } });
    const { user } = renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    await openFolderMenu(user, "Inbox");
    const item = await screen.findByRole("menuitem", { name: /Import mbox…: changes are turned off/ });
    expect(item.getAttribute("aria-disabled")).toBe("true");
    await user.click(item);
    expect(core.callsOf("import_mailbox_mbox")).toEqual([]);
    expect((await screen.findAllByText(/Changes are turned off for this account/)).length).toBeGreaterThan(0);
  });
});

const PREVIEW: TakeoverPreview = {
  token: "0".repeat(31) + "7",
  profileName: "t3stpr0f.default-release",
  accounts: [
    {
      index: 0,
      kind: "imap",
      name: "Alice (Thunderbird)",
      email: "alice@vicemail.test",
      displayName: "Alice Example",
      imap: { host: "127.0.0.1", port: 1993, security: "tls", username: "alice@vicemail.test" },
      smtp: { host: "127.0.0.1", port: 1465, security: "tls", username: "alice@vicemail.test" },
      wasUnencrypted: false,
      identities: [
        {
          email: "alice@vicemail.test",
          name: "Alice Example",
          replyTo: null,
          signature: { html: true, preview: "Alice\nExample Ltd.", fromFile: false },
          signatureUnreadable: false,
        },
      ],
      alreadySetUp: false,
    },
    {
      index: 1,
      kind: "pop3",
      name: "Bob",
      email: "bob@vicemail.test",
      displayName: "Bob Example",
      imap: null,
      smtp: null,
      wasUnencrypted: false,
      identities: [],
      alreadySetUp: false,
    },
  ],
  folders: [
    { index: 0, path: "Archives/2025", bytes: 2048 },
    { index: 1, path: "Inbox", bytes: 4096 },
  ],
};

describe("takeover from Thunderbird", () => {
  it("lists profiles, previews one and fills the account setup without a password", async () => {
    mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      stage19: { profiles: [{ index: 0, name: "default-release", isDefault: true }], previews: [PREVIEW] },
    });
    const { user } = renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    act(() => {
      useUiStore.getState().showAccountsPage({ kind: "list" });
    });
    await user.click(await screen.findByTestId("accounts-thunderbird"));
    const page = await screen.findByTestId("takeover");
    expect(page.textContent).toContain("The app never reads Thunderbird’s passwords");
    await user.click(within(page).getByTestId("takeover-profile-open"));
    const preview = await screen.findByTestId("takeover-preview");
    const accounts = within(preview).getAllByTestId("takeover-account");
    expect(accounts).toHaveLength(2);
    expect(accounts[0]?.textContent).toContain("Signature: Alice");
    expect(accounts[1]?.textContent).toContain("POP3 accounts cannot be taken over");
    expect(within(preview).getAllByTestId("takeover-folder").map((f) => f.textContent)).toEqual([
      expect.stringContaining("Archives/2025"),
      expect.stringContaining("Inbox"),
    ]);
    expect(await seriousFindings()).toEqual([]);
    await user.click(within(accounts[0] as HTMLElement).getByTestId("takeover-set-up"));
    const setup = await screen.findByTestId("takeover-setup");
    expect(within(setup).getByLabelText<HTMLInputElement>("Email address").value).toBe("alice@vicemail.test");
    expect(within(setup).getByLabelText<HTMLInputElement>("Password").value).toBe("");
    expect(within(setup).getAllByDisplayValue("127.0.0.1").length).toBeGreaterThan(0);
    expect(within(setup).getAllByDisplayValue("1993")).toHaveLength(1);
  });

  it("takes over addresses and signatures for an account set up already, and imports a folder", async () => {
    const alice = makeAccount({ id: "22222222-2222-4222-8222-222222222222", email: "alice@vicemail.test", writesEnabled: true });
    const core = mockCore({
      accounts: [alice],
      mailboxes: standardMailboxes(alice.id),
      stage19: { profiles: [{ index: 0, name: "default-release", isDefault: true }], previews: [PREVIEW] },
    });
    const { user } = renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    act(() => {
      useUiStore.getState().showAccountsPage({ kind: "thunderbird" });
    });
    await user.click(await screen.findByTestId("takeover-profile-open"));
    const preview = await screen.findByTestId("takeover-preview");
    await user.click(within(preview).getByTestId("takeover-apply"));
    expect((await within(preview).findByTestId("takeover-applied")).textContent).toContain("Addresses added: 1, signatures: 2.");
    expect(core.stage19.applied).toEqual([`${PREVIEW.token}/0/${alice.id}`]);
    const folder = within(preview).getAllByTestId("takeover-folder")[0] as HTMLElement;
    await waitFor(() => {
      expect(within(folder).getByTestId<HTMLSelectElement>("takeover-folder-mailbox").value).not.toBe("");
    });
    await user.click(within(folder).getByTestId("takeover-folder-import"));
    await waitFor(() => {
      expect(core.callsOf("import_thunderbird_folder")).toHaveLength(1);
    });
    expect(await screen.findByTestId("mbox-job")).toBeTruthy();
  });
});
