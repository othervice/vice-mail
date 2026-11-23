import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { IdentityInfo } from "../src/services/ipc";
import { ipcError, makeAccount, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

const ADA = makeAccount();
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", color: 3 });
const DEFAULT: IdentityInfo = { id: 1, accountId: ADA.id, email: "ada@vicemail.test", name: "Ada Lovelace", replyTo: null, isDefault: true };
const ALIAS: IdentityInfo = { id: 2, accountId: ADA.id, email: "info@vicemail.test", name: "Info", replyTo: "team@vicemail.test", isDefault: false };

async function accountsPage(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await user.click(screen.getByRole("tab", { name: "Accounts" }));
}

async function accountDetail(user: ReturnType<typeof renderApp>["user"]) {
  await accountsPage(user);
  await user.click(await screen.findByTestId(`account-row-${ADA.id}`));
  return screen.findByTestId("account-detail");
}

describe("account groups", () => {
  it("creates a group from a name and accounts, checks both, and lists it", async () => {
    const core = mockCore({ accounts: [ADA, WORK], mailboxes: standardMailboxes() });
    const { user } = renderApp();
    await accountsPage(user);
    const section = await screen.findByTestId("account-groups");
    expect(within(section).getByText("No groups yet.")).toBeTruthy();

    await user.click(within(section).getByRole("button", { name: "New group" }));
    await user.click(within(section).getByRole("button", { name: "Save group" }));
    expect(within(section).getByText("Enter a name.")).toBeTruthy();
    expect(within(section).getByText("Choose at least one account.")).toBeTruthy();
    expect(core.callsOf("create_account_group")).toEqual([]);

    await user.type(within(section).getByRole("textbox", { name: "Name of the group" }), "  Beruf ");
    await user.click(within(section).getByRole("checkbox", { name: "Work" }));
    await user.click(within(section).getByRole("button", { name: "Save group" }));
    await waitFor(() => {
      expect(core.callsOf("create_account_group")).toEqual([{ name: "Beruf", accountIds: [WORK.id] }]);
    });
    const group = await within(section).findByTestId("group");
    expect(within(group).getByText("Beruf")).toBeTruthy();
    expect(within(group).getByText("Work")).toBeTruthy();
    expect(core.search.groups.map((g) => g.name)).toEqual(["Beruf"]);
  });

  it("changes the members of a group and deletes it", async () => {
    const core = mockCore({
      accounts: [ADA, WORK],
      mailboxes: standardMailboxes(),
      search: { groups: [{ id: "g1", name: "Alle", accountIds: [ADA.id], position: 0 }] },
    });
    const { user } = renderApp();
    await accountsPage(user);
    const section = await screen.findByTestId("account-groups");
    await user.click(await within(section).findByRole("button", { name: "Edit Alle" }));
    await user.click(within(section).getByRole("checkbox", { name: "Work" }));
    await user.click(within(section).getByRole("button", { name: "Save group" }));
    await waitFor(() => {
      expect(core.callsOf("update_account_group")).toEqual([{ groupId: "g1", name: "Alle", accountIds: [ADA.id, WORK.id] }]);
    });
    await user.click(await within(section).findByRole("button", { name: "Delete Alle" }));
    await waitFor(() => {
      expect(core.callsOf("delete_account_group")).toEqual([{ groupId: "g1" }]);
    });
    expect(await within(section).findByText("No groups yet.")).toBeTruthy();
  });

  it("shows what the core refused", async () => {
    mockCore({ accounts: [ADA], mailboxes: standardMailboxes(), failures: { create_account_group: ipcError("invalid_argument") } });
    const { user } = renderApp();
    await accountsPage(user);
    const section = await screen.findByTestId("account-groups");
    await user.click(within(section).getByRole("button", { name: "New group" }));
    await user.type(within(section).getByRole("textbox", { name: "Name of the group" }), "X");
    await user.click(within(section).getByRole("checkbox", { name: "Ada Lovelace" }));
    await user.click(within(section).getByRole("button", { name: "Save group" }));
    const alert = await within(section).findByRole("alert");
    expect(within(alert).getByText("The group could not be saved")).toBeTruthy();
    expect(alert.dataset["errorCode"]).toBe("invalid_argument");
  });
});

describe("hiding an account from the unified inbox", () => {
  it("is a switch in the account's settings that asks the core", async () => {
    const core = mockCore({ accounts: [ADA, WORK], mailboxes: standardMailboxes() });
    const { user } = renderApp();
    const detail = await accountDetail(user);
    const toggle = await within(detail).findByRole("switch", { name: "Show in the unified inbox" });
    await waitFor(() => {
      expect(toggle.getAttribute("aria-checked")).toBe("true");
    });
    await user.click(toggle);
    await waitFor(() => {
      expect(core.callsOf("set_account_hidden")).toEqual([{ accountId: ADA.id, hidden: true }]);
    });
    await waitFor(() => {
      expect(toggle.getAttribute("aria-checked")).toBe("false");
    });
    expect(core.search.hidden).toEqual([ADA.id]);
  });
});

describe("identities", () => {
  it("lists the identities with the default marked; the default cannot be deleted", async () => {
    mockCore({ accounts: [ADA], mailboxes: standardMailboxes(), search: { identities: [DEFAULT, ALIAS] } });
    const { user } = renderApp();
    const detail = await accountDetail(user);
    const list = await within(detail).findByRole("list", { name: "Identities of Ada Lovelace" });
    const rows = within(list).getAllByTestId("identity");
    expect(rows.map((r) => r.querySelector("span")?.textContent)).toEqual([
      "Ada Lovelace (ada@vicemail.test)",
      "Info (info@vicemail.test)",
    ]);
    expect(within(rows[0] as HTMLElement).getByTestId("identity-default").textContent).toBe("Default");
    expect(within(rows[1] as HTMLElement).getByText("Replies go to team@vicemail.test")).toBeTruthy();
    const deleteDefault = within(rows[0] as HTMLElement).getByRole("button", { name: "Delete ada@vicemail.test" });
    expect((deleteDefault as HTMLButtonElement).disabled).toBe(true);
    expect(document.getElementById(deleteDefault.getAttribute("aria-describedby") ?? "")?.textContent).toBe(
      "Make another identity the default before deleting this one.",
    );
  });

  it("adds an identity, makes it the default and deletes the old one", async () => {
    const core = mockCore({ accounts: [ADA], mailboxes: standardMailboxes(), search: { identities: [DEFAULT] } });
    const { user } = renderApp();
    const detail = await accountDetail(user);
    await user.click(await within(detail).findByRole("button", { name: "Add identity" }));
    const form = within(detail).getByTestId("identity-form");
    await user.click(within(form).getByRole("button", { name: "Save identity" }));
    expect(within(form).getByText("Enter an email address.")).toBeTruthy();
    await user.type(within(form).getByRole("textbox", { name: "Address" }), "hello@vicemail.test");
    await user.type(within(form).getByRole("textbox", { name: "Name" }), "Hello");
    await user.click(within(form).getByRole("button", { name: "Save identity" }));
    await waitFor(() => {
      expect(core.callsOf("add_identity")).toEqual([
        { accountId: ADA.id, identity: { email: "hello@vicemail.test", name: "Hello", replyTo: null } },
      ]);
    });

    await user.click(await within(detail).findByRole("button", { name: "Make hello@vicemail.test the default" }));
    await waitFor(() => {
      expect(core.search.identities.find((i) => i.email === "hello@vicemail.test")?.isDefault).toBe(true);
    });
    const remove = await within(detail).findByRole("button", { name: "Delete ada@vicemail.test" });
    await waitFor(() => {
      expect((remove as HTMLButtonElement).disabled).toBe(false);
    });
    await user.click(remove);
    await waitFor(() => {
      expect(core.callsOf("delete_identity")).toEqual([{ identityId: DEFAULT.id }]);
    });
    await waitFor(() => {
      expect(within(detail).getAllByTestId("identity")).toHaveLength(1);
    });
  });

  it("shows a refused address at the form and reloads after accountsChanged", async () => {
    const core = mockCore({ accounts: [ADA], mailboxes: standardMailboxes(), search: { identities: [DEFAULT] } });
    const { user } = renderApp();
    const detail = await accountDetail(user);
    await user.click(await within(detail).findByRole("button", { name: "Add identity" }));
    const form = within(detail).getByTestId("identity-form");
    // Same address as the default (the core compares without case).
    await user.type(within(form).getByRole("textbox", { name: "Address" }), "ADA@vicemail.test");
    await user.click(within(form).getByRole("button", { name: "Save identity" }));
    const alert = await within(form).findByRole("alert");
    expect(within(alert).getByText("The identity could not be saved")).toBeTruthy();

    const before = core.callsOf("list_identities").length;
    await emitCore({ type: "accountsChanged" });
    await waitFor(() => {
      expect(core.callsOf("list_identities").length).toBeGreaterThan(before);
    });
  });
});
