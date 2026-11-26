import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { accountFill } from "../src/features/accounts/accountColor";
import type { AccountGroup, InboxScope } from "../src/services/ipc";
import { ipcError, makeAccount, makeMailbox, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";
import { showHeaderDetails } from "./support/reading";
import { optionName } from "./support/names";

const ADA = makeAccount();
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", color: 3 });
const CLUB = makeAccount({ id: "44444444-4444-4444-8444-444444444444", email: "club@example.net", displayName: "Club", color: 5 });
const now = Date.now();
const ADA_MAIL = makeMessage({ id: 3000, accountId: ADA.id, mailboxId: 1, subject: "Privat", date: now - 1000 * 60 });
const WORK_MAIL = makeMessage({ id: 2000, accountId: WORK.id, mailboxId: 20, subject: "Projekt", date: now - 1000 * 120 });
const CLUB_MAIL = makeMessage({ id: 1000, accountId: CLUB.id, mailboxId: 30, subject: "Training", date: now - 1000 * 180 });
const GROUP: AccountGroup = { id: "55555555-5555-4555-8555-555555555555", name: "Freizeit", accountIds: [ADA.id, CLUB.id], position: 0 };

function setup(options: Parameters<typeof mockCore>[0] = {}) {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  const core = mockCore({
    accounts: [ADA, WORK, CLUB],
    mailboxes: [
      ...standardMailboxes(),
      makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" }),
      makeMailbox({ id: 30, accountId: CLUB.id, path: "INBOX", role: "inbox", roleSource: "server" }),
    ],
    messages: { 1: [ADA_MAIL], 20: [WORK_MAIL], 30: [CLUB_MAIL] },
    views: Object.fromEntries([ADA_MAIL, WORK_MAIL, CLUB_MAIL].map((m) => [m.id, makeView(m)])),
    ...options,
  });
  return { core, ...renderApp() };
}

async function choose(user: ReturnType<typeof renderApp>["user"], name: string) {
  await user.click(await screen.findByRole("button", { name: /^Show the inbox of/ }));
  const choices = await screen.findByRole("list", { name: "Show the inbox of" });
  await user.click(within(choices).getByRole("button", { name }));
}

const subjects = (list: HTMLElement) =>
  within(list)
    .getAllByRole("option")
    .map((option) => optionName(option).split(", ")[1]);

describe("unified inbox", () => {
  it("mixes the inboxes of all accounts by time; every row keeps its account and colour", async () => {
    const { core, user } = setup();
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await choose(user, "All accounts");

    const list = await screen.findByRole("listbox", { name: "Messages in All inboxes" });
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("All inboxes");
    await waitFor(() => {
      expect(within(list).getAllByRole("option")).toHaveLength(3);
    });
    // Stage 9's sorted inbox with the unified scope (blocks, bundles, hidden snoozes).
    expect(core.callsOf("list_inbox")).toContainEqual({ scope: { kind: "unified", view: { kind: "all" } }, cursor: null, limit: 200 });
    expect(core.callsOf("list_unified_inbox")).toEqual([]);
    const options = within(list).getAllByRole("option");
    // Stage 22 (decision 0072): no avatar ring; a dot in the account's colour before the sender,
    // named by its tooltip, and the account in words in the row's name.
    for (const [index, account] of [ADA, WORK, CLUB].entries()) {
      const option = options[index] as HTMLElement;
      const dot = within(option).getByTestId("row-account-dot");
      expect(dot.title).toBe(account.displayName || account.email);
      expect(dot.querySelector(`.${accountFill(account.color)}`)).toBeTruthy();
      expect(within(option).queryByTestId("avatar")).toBeNull();
    }

    // Opening a row opens it with its own account.
    await user.click(options[1] as HTMLElement);
    const pane = await screen.findByRole("region", { name: "Message" });
    await showHeaderDetails(user, pane);
    expect(within(pane).getByTestId("reading-account").textContent).toBe("Work (work@example.com)");
    // Stage 18 (task F): the unified inbox showed its top message first; the click opened this one.
    expect(core.callsOf("get_message").at(-1)).toEqual({ messageId: WORK_MAIL.id, format: "auto", scheme: "light" });
  });

  it("acts on an opened row with the actions of its own account", async () => {
    const writable = { ...WORK, writesEnabled: true };
    const { core, user } = setup({
      accounts: [ADA, writable, CLUB],
      mailboxes: [
        ...standardMailboxes(),
        makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" }),
        makeMailbox({ id: 21, accountId: WORK.id, path: "Archive", role: "archive", roleSource: "server" }),
        makeMailbox({ id: 30, accountId: CLUB.id, path: "INBOX", role: "inbox", roleSource: "server" }),
      ],
    });
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await choose(user, "All accounts");
    const list = await screen.findByRole("listbox", { name: "Messages in All inboxes" });
    await waitFor(() => {
      expect(within(list).getAllByRole("option")).toHaveLength(3);
    });
    await user.click(within(list).getAllByRole("option")[1] as HTMLElement);
    const pane = await screen.findByRole("region", { name: "Message" });
    const toolbar = within(pane).getByRole("toolbar", { name: "Message actions" });
    // No pickers or dialogs of the folder view here.
    expect(within(toolbar).queryByRole("button", { name: /^Move/ })).toBeNull();
    await user.click(within(toolbar).getByRole("button", { name: /^Archive/ }));
    await waitFor(() => {
      expect(core.callsOf("apply_action")).toContainEqual(
        expect.objectContaining({ action: { type: "archive" }, messages: [{ mailboxId: 20, messageId: WORK_MAIL.id }] }),
      );
    });
  });

  it("takes the action shortcuts for the open row, and explains them on a read-only account", async () => {
    const writable = { ...WORK, writesEnabled: true };
    const { core, user } = setup({
      accounts: [ADA, writable, CLUB],
      mailboxes: [
        ...standardMailboxes(),
        makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" }),
        makeMailbox({ id: 21, accountId: WORK.id, path: "Archive", role: "archive", roleSource: "server" }),
        makeMailbox({ id: 30, accountId: CLUB.id, path: "INBOX", role: "inbox", roleSource: "server" }),
      ],
      readingPreferences: { markReadOnOpen: "never" },
    });
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await choose(user, "All accounts");
    const list = await screen.findByRole("listbox", { name: "Messages in All inboxes" });
    await waitFor(() => {
      expect(within(list).getAllByRole("option")).toHaveLength(3);
    });
    await user.click(within(list).getAllByRole("option")[1] as HTMLElement);
    await screen.findByRole("region", { name: "Message" });
    await user.keyboard("e");
    await waitFor(() => {
      expect(core.callsOf("apply_action")).toEqual([
        expect.objectContaining({ action: { type: "archive" }, messages: [{ mailboxId: 20, messageId: WORK_MAIL.id }] }),
      ]);
    });

    // Ada's account is read-only: the shortcut explains instead of doing nothing.
    await user.click(within(list).getAllByRole("option")[0] as HTMLElement);
    await screen.findByRole("region", { name: "Message" });
    await user.keyboard("e");
    await waitFor(() => {
      expect(screen.getAllByTestId("notice").map((n) => n.dataset["noticeKind"])).toContain("readOnly");
    });
    expect(core.callsOf("apply_action")).toHaveLength(1);
  });

  it("leaves hidden accounts out", async () => {
    const { user } = setup({ search: { hidden: [WORK.id] } });
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await choose(user, "All accounts");
    const list = await screen.findByRole("listbox", { name: "Messages in All inboxes" });
    await waitFor(() => {
      expect(subjects(list)).toEqual(["Privat", "Training"]);
    });
  });

  it("says so when every account is hidden, with the way to the settings", async () => {
    const { user } = setup({ search: { hidden: [ADA.id, WORK.id, CLUB.id] } });
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await choose(user, "All accounts");
    expect(await screen.findByText("All accounts are hidden")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Open account settings" }));
    expect(await screen.findByRole("tab", { name: "Accounts", selected: true })).toBeTruthy();
  });

  it("shows a group's inboxes, from the header and from the sidebar", async () => {
    const { core, user } = setup({ search: { groups: [GROUP] } });
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await choose(user, "Freizeit");
    const list = await screen.findByRole("listbox", { name: "Messages in Freizeit" });
    await waitFor(() => {
      expect(subjects(list)).toEqual(["Privat", "Training"]);
    });
    expect(core.callsOf("list_inbox")).toContainEqual({
      scope: { kind: "unified", view: { kind: "group", groupId: GROUP.id } },
      cursor: null,
      limit: 200,
    });

    // Back to one account through the switcher.
    await choose(user, "Work (work@example.com)");
    await waitFor(() => {
      expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Inbox");
    });

    // The column lists the unified inbox (with its unread count in the name) and the groups.
    await user.click(screen.getByTestId("rail-folders"));
    const tree = await screen.findByRole("region", { name: "Inboxes" });
    await user.click(within(tree).getByRole("treeitem", { name: /^All inboxes/ }));
    expect(await screen.findByRole("listbox", { name: "Messages in All inboxes" })).toBeTruthy();
    expect(within(tree).getByRole("treeitem", { name: /^All inboxes/ }).getAttribute("aria-current")).toBe("page");
  });

  it("stays a sorted inbox in the conversation view (decision 0039): the sorted inbox as conversations", async () => {
    const { core, user } = setup();
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await choose(user, "All accounts");
    await screen.findByRole("listbox", { name: "Messages in All inboxes" });
    const toggle = screen.getByRole("button", { name: "Conversations" });
    expect(toggle.getAttribute("aria-pressed")).toBe("false");
    await user.click(toggle);
    await waitFor(() => {
      expect(core.callsOf("list_inbox_conversations")).toContainEqual({
        scope: { kind: "unified", view: { kind: "all" } },
        cursor: null,
        limit: 200,
      });
    });
    expect(core.callsOf("list_unified_threads")).toEqual([]);
  });

  it("shows a failed load at its place with a retry", async () => {
    const { user } = setup({
      fail: (cmd, args) =>
        cmd === "list_inbox" && (args["scope"] as InboxScope).kind === "unified" ? ipcError("database") : undefined,
    });
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await choose(user, "All accounts");
    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText("The unified inbox could not be loaded")).toBeTruthy();
    expect(within(alert).getByRole("button", { name: "Try again" })).toBeTruthy();
  });

  it("is not offered with a single account", async () => {
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
    mockCore({ accounts: [ADA], mailboxes: standardMailboxes(), messages: { 1: [ADA_MAIL] } });
    renderApp();
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    expect(screen.queryByRole("button", { name: /^Show the inbox of/ })).toBeNull();
  });
});
