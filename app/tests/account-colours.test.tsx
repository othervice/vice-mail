import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { accountShortNames } from "../src/features/accounts/accountName";
import type { Account } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, makeMailbox, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { makeOutgoing } from "./support/compose";
import { renderApp } from "./support/render";

/*
 * Stage 8, "Kontofarben immer mit Namen" (docs/SPEC.md: no information by colour alone). Every
 * element in an account colour (avatar ring, dot) has the account's name next to it: in its own
 * row, button or label (text or accessible name). A list of a single account may leave the name
 * out of its rows, as long as the page names that account next to the same colour (rail, header).
 */

const ADA = makeAccount();
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", color: 3 });
const CLUB = makeAccount({ id: "44444444-4444-4444-8444-444444444444", email: "club@example.net", displayName: "Club", color: 5 });
const now = Date.now();
const ADA_MAIL = makeMessage({ id: 3000, accountId: ADA.id, mailboxId: 1, subject: "Privat", date: now - 60_000 });
const WORK_MAIL = makeMessage({ id: 2000, accountId: WORK.id, mailboxId: 20, subject: "Projekt", date: now - 120_000 });
const CLUB_MAIL = makeMessage({ id: 1000, accountId: CLUB.id, mailboxId: 30, subject: "Training", date: now - 180_000 });

const COLOUR = /\b(?:ring|bg)-account-([1-8])\b/;

function colourOf(element: Element): string {
  return COLOUR.exec(element.getAttribute("class") ?? "")?.[1] ?? "";
}

/** Elements in an account colour without the account's name next to them. */
function unnamedColours(accounts: readonly Account[]): string[] {
  const names = accounts.flatMap((account) => [account.displayName.trim(), account.email].filter(Boolean));
  const coloured = [...document.body.querySelectorAll("[class]")].filter((element) => COLOUR.test(element.getAttribute("class") ?? ""));
  const named = (element: Element) => {
    let node: Element | null = element;
    for (let level = 0; level < 5 && node && node !== document.body; level += 1, node = node.parentElement) {
      const text = [node.getAttribute("aria-label"), node.getAttribute("title"), node.textContent].join(" ");
      if (names.some((name) => text.includes(name))) return true;
    }
    return false;
  };
  const namedColours = new Set(coloured.filter(named).map(colourOf));
  return coloured
    .filter((element) => {
      if (named(element)) return false;
      const list = element.closest('[role="listbox"]');
      const inList = list ? coloured.filter((other) => list.contains(other)) : [];
      const oneAccount = inList.length > 0 && new Set(inList.map(colourOf)).size === 1;
      return !(oneAccount && namedColours.has(colourOf(element)));
    })
    .map((element) => `${element.tagName.toLowerCase()}.account-${colourOf(element)} in "${(element.parentElement?.textContent ?? "").slice(0, 60)}"`);
}

function setup(accounts: Account[] = [ADA, WORK, CLUB]) {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  const core = mockCore({
    accounts,
    mailboxes: [
      ...standardMailboxes(),
      makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" }),
      makeMailbox({ id: 30, accountId: CLUB.id, path: "INBOX", role: "inbox", roleSource: "server" }),
    ],
    messages: { 1: [ADA_MAIL], 20: [WORK_MAIL], 30: [CLUB_MAIL] },
    views: Object.fromEntries([ADA_MAIL, WORK_MAIL, CLUB_MAIL].map((m) => [m.id, makeView(m)])),
    readingPreferences: { markReadOnOpen: "never" },
    compose: {
      outgoing: [
        makeOutgoing({ operationId: 70, accountId: ADA.id, subject: "Hello" }),
        makeOutgoing({ operationId: 71, accountId: WORK.id, subject: "Report" }),
      ],
    },
  });
  return { core, ...renderApp() };
}

async function showAllInboxes(user: ReturnType<typeof renderApp>["user"]) {
  await screen.findByRole("listbox", { name: "Messages in Inbox" });
  await user.click(await screen.findByRole("button", { name: /^Show the inbox of/ }));
  const choices = await screen.findByRole("list", { name: "Show the inbox of" });
  expect(unnamedColours([ADA, WORK, CLUB])).toEqual([]);
  await user.click(within(choices).getByRole("button", { name: "All accounts" }));
  return screen.findByRole("listbox", { name: "Messages in All inboxes" });
}

describe("account colours", () => {
  it("name an account by its display name, or by its address where the name is empty or shared", () => {
    const private_ = makeAccount({ id: "a", email: "ada@home.example", displayName: "Ada" });
    const job = makeAccount({ id: "b", email: "ada@job.example", displayName: "Ada" });
    const plain = makeAccount({ id: "c", email: "info@example.org", displayName: "  " });
    const club = makeAccount({ id: "d", email: "club@example.net", displayName: "Club" });
    expect([...accountShortNames([private_, job, plain, club]).values()]).toEqual([
      "ada@home.example",
      "ada@job.example",
      "info@example.org",
      "Club",
    ]);
  });

  it("come with the account's name in every row of the unified inbox, seen and heard", async () => {
    const { user } = setup();
    const list = await showAllInboxes(user);
    await waitFor(() => {
      expect(within(list).getAllByRole("option")).toHaveLength(3);
    });
    const options = within(list).getAllByRole("option");
    expect(options.map((option) => within(option).getByTestId("row-account").textContent)).toEqual(["Ada Lovelace", "Work", "Club"]);
    expect(options[1]?.getAttribute("aria-label")).toContain("Projekt, Account: Work");
    expect(unnamedColours([ADA, WORK, CLUB])).toEqual([]);
    // The opened message names its account in the header, next to the dot.
    await user.click(options[1] as HTMLElement);
    await screen.findByTestId("reading-account");
    expect(unnamedColours([ADA, WORK, CLUB])).toEqual([]);
  });

  it("need no name in the rows while only one account is shown", async () => {
    const { user } = setup([ADA]);
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    act(() => {
      useUiStore.getState().showUnifiedInbox({ kind: "all" });
    });
    const list = await screen.findByRole("listbox", { name: "Messages in All inboxes" });
    await waitFor(() => {
      expect(within(list).getAllByRole("option")).toHaveLength(1);
    });
    expect(within(list).queryByTestId("row-account")).toBeNull();
    expect(unnamedColours([ADA])).toEqual([]);
    await user.click(within(list).getAllByRole("option")[0] as HTMLElement);
    await screen.findByTestId("reading-account");
    expect(unnamedColours([ADA])).toEqual([]);
  });

  it("come with the account's name in search results that mix accounts", async () => {
    const { user } = setup();
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.type(await screen.findByRole("searchbox", { name: "Search mail" }), "r");
    const results = await screen.findByRole("listbox", { name: /search results/ });
    const options = await within(results).findAllByRole("option");
    expect(options.length).toBeGreaterThan(0);
    for (const option of options) {
      expect(within(option).getByTestId("row-account").textContent).toMatch(/^(Ada Lovelace|Work|Club)$/);
      expect(option.getAttribute("aria-label")).toMatch(/Account: (Ada Lovelace|Work|Club)/);
    }
    expect(unnamedColours([ADA, WORK, CLUB])).toEqual([]);
  });

  it("come with a name in the folder view, the rail, the sidebar, activity, outbox and settings", async () => {
    const { user } = setup();
    await screen.findByRole("listbox", { name: "Messages in Inbox" });
    expect(unnamedColours([ADA, WORK, CLUB])).toEqual([]);
    await user.click(screen.getByTestId("rail-folders"));
    await screen.findByTestId("mailbox-5");
    expect(unnamedColours([ADA, WORK, CLUB])).toEqual([]);
    act(() => {
      useUiStore.getState().showActivity();
    });
    await screen.findByRole("heading", { level: 1, name: "Activity" });
    expect(unnamedColours([ADA, WORK, CLUB])).toEqual([]);
    act(() => {
      useUiStore.getState().showOutbox();
    });
    await screen.findAllByTestId("outgoing-row");
    expect(unnamedColours([ADA, WORK, CLUB])).toEqual([]);
    for (const section of ["accounts", "reading"] as const) {
      act(() => {
        useUiStore.getState().showSettings(section);
      });
      await screen.findByRole("tab", { name: section === "accounts" ? "Accounts" : "Reading", selected: true });
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
      expect(unnamedColours([ADA, WORK, CLUB]), section).toEqual([]);
    }
  });
});
