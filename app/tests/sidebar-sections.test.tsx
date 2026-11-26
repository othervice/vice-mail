import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";
import { syncSummary } from "../src/app/SidebarFooter";
import { defaultOpen, withFold } from "../src/features/layout/sidebarFolds";
import { buildMailboxTree, splitAccountTree } from "../src/features/mailboxes/tree";
import { createI18n } from "../src/i18n";
import type { AccountStatus } from "../src/services/ipc";
import { useDragStore } from "../src/stores/drag";
import { useUiStore } from "../src/stores/ui";
import { IDLE, makeAccount, makeMailbox, makeMessage, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";
import { DEFAULT_UI_PREFERENCES } from "./support/uiPrefs";

/*
 * Stage 23, item 2, step 2 (decision 0085): the sidebar of the approved draft. "All accounts"
 * (the unified inbox, Later, Flagged, Sent, "More"), favourites with icons in their account's
 * colour, one section per account (heading, inbox, Sent, "More folders"), the foot with the sync
 * state, "Sync now" and Settings; what is opened or closed is remembered on the device.
 */

const ADA = makeAccount({ writesEnabled: true });
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", color: 3, writesEnabled: true });
const CLUB = makeAccount({ id: "44444444-4444-4444-8444-444444444444", email: "club@example.net", displayName: "Club", color: 5 });
const t = createI18n("en").t;
const tDe = createI18n("de").t;

const status = (overrides: Partial<AccountStatus>): AccountStatus => ({ ...IDLE, ...overrides });

/** axe-core as in tests/a11y.test.tsx (colour contrast: tests/contrast.test.ts). */
async function seriousFindings(): Promise<string[]> {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  const results = await axe.run(document.body, { resultTypes: ["violations"], iframes: false, rules: { "color-contrast": { enabled: false } } });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}

const LAYOUT = { readingPane: "right" as const, density: "comfortable" as const, sidebarExpanded: true, sidebarWidth: 240, listWidth: 384, listHeight: 320, contactPanel: true };

describe("an account's section", () => {
  it("has the inbox and Sent on their own; everything else under More folders, roles first, own folders by name", () => {
    const mailboxes = [
      makeMailbox({ id: 1, path: "INBOX", role: "inbox" }),
      makeMailbox({ id: 2, path: "INBOX/Sent", role: "sent" }),
      makeMailbox({ id: 3, path: "INBOX/Trash", role: "trash" }),
      makeMailbox({ id: 4, path: "INBOX/Zeitung", parentId: 1 }),
      makeMailbox({ id: 5, path: "Archiv 2020" }),
      makeMailbox({ id: 6, path: "INBOX/Belege", parentId: 1 }),
      makeMailbox({ id: 7, path: "INBOX/Belege/2026", parentId: 6 }),
      makeMailbox({ id: 8, path: "INBOX/Sent/Alt", parentId: 2 }),
    ];
    const split = splitAccountTree(buildMailboxTree(mailboxes));
    expect(split.inbox?.id).toBe(1);
    expect(split.sent?.id).toBe(2);
    // Folders kept under INBOX (servers with `INBOX.` for everything) come up among the own ones.
    expect(split.more.map((node) => node.mailbox.name)).toEqual(["Trash", "Alt", "Archiv 2020", "Belege", "Zeitung"]);
    expect(split.more.find((node) => node.mailbox.id === 6)?.children.map((node) => node.mailbox.id)).toEqual([7]);
    expect(splitAccountTree([])).toEqual({ inbox: undefined, sent: undefined, more: [] });
  });
});

describe("what is opened or closed", () => {
  it("keeps only what differs from the default, drops parts of removed accounts and stays in the core's limit", () => {
    expect(defaultOpen("more")).toBe(false);
    expect(defaultOpen(`folders:${ADA.id}`)).toBe(false);
    expect(defaultOpen("favorites")).toBe(true);
    expect(defaultOpen(`account:${ADA.id}`)).toBe(true);
    const opened = withFold([], "more", true, null);
    expect(opened).toEqual([{ key: "more", open: true }]);
    // Back to the default: no entry.
    expect(withFold(opened, "more", false, null)).toEqual([]);
    const gone = withFold([{ key: `account:${WORK.id}`, open: false }, { key: "favorites", open: false }], `folders:${ADA.id}`, true, new Set([ADA.id]));
    expect(gone).toEqual([
      { key: "favorites", open: false },
      { key: `folders:${ADA.id}`, open: true },
    ]);
    const many = Array.from({ length: 128 }, (_, i) => ({ key: `account:a${String(i)}`, open: false }));
    const next = withFold(many, "more", true, null);
    expect(next).toHaveLength(128);
    expect(next.at(-1)).toEqual({ key: "more", open: true });
    expect(next[0]?.key).toBe("account:a1");
  });
});

describe("the foot's line", () => {
  const now = new Date(2026, 10, 26, 18, 30).getTime();
  const at = new Date(2026, 10, 26, 18, 4).getTime();

  it("says when the last account finished a pass, with the date if not today", () => {
    expect(syncSummary([], t, "en", now)).toBeNull();
    const a = makeAccount({ status: status({ lastSyncAt: at }) });
    const b = makeAccount({ id: WORK.id, status: status({ lastSyncAt: at + 120_000 }) });
    expect(syncSummary([a, b], t, "en", now)?.text).toBe(`Synced at ${new Intl.DateTimeFormat("en", { timeStyle: "short" }).format(at)}`);
    expect(syncSummary([a, b], tDe, "de", now)?.text).toBe("Abgeglichen um 18:04");
    const yesterday = at - 24 * 3600 * 1000;
    expect(syncSummary([makeAccount({ status: status({ lastSyncAt: yesterday }) })], tDe, "de", now)?.text).toBe("Abgeglichen am 25.11. um 18:04");
    // Never synced yet and idle: nothing to say.
    expect(syncSummary([makeAccount({ status: status({ lastSyncAt: null }) })], t, "en", now)).toBeNull();
  });

  it("names an account that does not sync before one that is offline, then syncing, with the list header's words", () => {
    const syncing = makeAccount({ status: status({ state: "syncing" }) });
    expect(syncSummary([syncing, WORK], t, "en", now)).toMatchObject({ text: "Syncing…", tone: "quiet" });
    const offline = makeAccount({ id: CLUB.id, displayName: "Club", status: status({ state: "offline" }) });
    expect(syncSummary([syncing, offline], t, "en", now)).toMatchObject({ text: "Club: Offline", tone: "offline" });
    const failed = makeAccount({ id: WORK.id, displayName: "Work", status: status({ state: "error" }) });
    expect(syncSummary([failed, offline], t, "en", now)).toMatchObject({ text: "Work: Not syncing", tone: "error", detail: "Not syncing: Work" });
    const second = makeAccount({ id: "55555555-5555-4555-8555-555555555555", email: "x@example.org", displayName: "", status: status({ state: "error" }) });
    expect(syncSummary([failed, second], tDe, "de", now)).toMatchObject({ text: "2 Konten: Kein Abgleich", detail: "Kein Abgleich: Work, x@example.org" });
    expect(syncSummary([offline, { ...ADA, status: status({ state: "offline" }) }], t, "en", now)).toMatchObject({
      text: "Offline",
      detail: "Offline. Your mail on this computer stays available.",
    });
  });
});

describe("the sidebar of the draft", () => {
  function setup(options: Parameters<typeof mockCore>[0] = {}) {
    const core = mockCore({
      accounts: [ADA, WORK],
      mailboxes: [
        ...standardMailboxes().map((mailbox) => (mailbox.id === 5 ? { ...mailbox, favoritePosition: 0 } : mailbox)),
        makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server", unread: 2 }),
        makeMailbox({ id: 21, accountId: WORK.id, path: "Projekte", favoritePosition: 1 }),
        makeMailbox({ id: 22, accountId: WORK.id, path: "Sent", role: "sent", roleSource: "server" }),
      ],
      ui: { preferences: DEFAULT_UI_PREFERENCES, layout: LAYOUT },
      ...options,
    });
    return { core, ...renderApp(options.preferences?.language === "de" ? "de" : "en") };
  }

  it("gives favourites and an account's folders the account's colour; a favourite names its account only where its name is not unique", async () => {
    setup();
    const favorites = await screen.findByRole("tree", { name: "Favourites" });
    const rows = await within(favorites).findAllByTestId(/^favorite-/);
    expect(rows.map((row) => row.getAttribute("aria-label"))).toEqual(["Projekte in Ada Lovelace, 2 unread", "Projekte in Work"]);
    // Both are called Projekte: each shows its account's short name too.
    expect(rows.map((row) => row.textContent)).toEqual(["ProjekteAda Lovelace2", "ProjekteWork"]);
    expect(rows[1]?.querySelector("svg")?.getAttribute("class")).toContain("text-account-4");
    const inbox = await screen.findByTestId("mailbox-20");
    expect(inbox.querySelector("svg")?.getAttribute("class")).toContain("text-account-4");
    // The heading: a square in the colour, the name beside it.
    const heading = screen.getByTestId(`sidebar-account-${WORK.id}`);
    expect(heading.querySelector(".bg-account-4")?.getAttribute("class")).toContain("rounded-mark");
    expect(heading.textContent).toBe("Work");
  });

  it("names the account's unread mail on its dot in the narrow rail", async () => {
    const { user } = setup();
    await screen.findByRole("tree", { name: "All accounts" });
    await user.click(screen.getByRole("button", { name: "Hide sidebar" }));
    const work = await screen.findByTestId(`account-switch-${WORK.id}`);
    await waitFor(() => {
      expect(work.getAttribute("aria-label")).toBe("Work (work@example.com), up to date, 2 unread");
    });
    expect(within(work).getByTestId(`account-count-${WORK.id}`).textContent).toBe("2");
    // The rail's groups: the inbox, the first section's views and Sent; a hairline; the rest.
    expect(within(screen.getByTestId("icon-rail-views-main")).getAllByRole("button").map((button) => button.dataset["testid"])).toEqual([
      "rail-inbox",
      "rail-later",
      "rail-flagged",
      "rail-sent",
    ]);
    expect(await seriousFindings()).toEqual([]);
  });

  it("opens More folders while dragged mail rests on it", async () => {
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
    const mail = makeMessage({ id: 3000, accountId: ADA.id, mailboxId: 1, subject: "Rechnung" });
    setup({ messages: { 1: [mail] } });
    const more = await screen.findByTestId(`sidebar-more-folders-${ADA.id}`);
    expect(more.getAttribute("aria-expanded")).toBe("false");
    act(() => {
      useDragStore.getState().start({ items: [mail], count: 1, drop: vi.fn() });
    });
    const data = new Map<string, string>();
    const transfer = {
      types: ["application/x-mail-rows"],
      getData: (type: string) => data.get(type) ?? "",
      setData: (type: string, value: string) => data.set(type, value),
      dropEffect: "none",
      effectAllowed: "copyMove",
    };
    const started = Date.now();
    while (Date.now() - started < 1000) {
      fireEvent.dragOver(more, { dataTransfer: transfer });
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 100));
      });
    }
    expect(await screen.findByTestId("mailbox-5")).toBeTruthy();
    expect(more.getAttribute("aria-expanded")).toBe("true");
    act(() => {
      useDragStore.getState().end();
    });
  }, 10_000);

  it("speaks German: Alle Konten, Mehr, Weitere Ordner, the foot", async () => {
    setup({ preferences: { theme: "light", language: "de" } });
    const nav = await screen.findByRole("navigation", { name: "Konten und Ordner" });
    const all = await within(nav).findByRole("tree", { name: "Alle Konten" });
    expect(within(all).getByRole("treeitem", { name: /^Posteingang/ })).toBeTruthy();
    expect(within(all).getByRole("treeitem", { name: "Mehr" })).toBeTruthy();
    expect(within(nav).getByTestId(`sidebar-more-folders-${ADA.id}`).textContent).toContain("Weitere Ordner");
    expect(within(nav).getByTestId("sidebar-sync-status").textContent).toMatch(/^Abgeglichen am /);
    expect(within(nav).getByRole("toolbar", { name: "Abgleich und Einstellungen" })).toBeTruthy();
    expect(within(nav).getByRole("button", { name: "Jetzt abgleichen" })).toBeTruthy();
    act(() => {
      useUiStore.getState().showAllAccountsView("later");
    });
    expect(within(all).getByRole("treeitem", { name: "Später" }).getAttribute("aria-current")).toBe("page");
    expect(await seriousFindings()).toEqual([]);
  });

  it("a failed action shows on a closed More and on the folded section", async () => {
    setup({
      operations: [
        {
          id: 1,
          batch: 1,
          accountId: ADA.id,
          action: "archive",
          kind: "move",
          state: "failed",
          mailboxId: 1,
          messageId: 100,
          targetMailboxId: 4,
          messageCount: null,
          attempts: 3,
          error: { code: "connection_lost", message: "no", detail: null },
          conflict: null,
          notice: null,
          createdAt: Date.now(),
          updatedAt: Date.now(),
        },
      ],
    });
    const all = await screen.findByRole("tree", { name: "All accounts" });
    const more = within(all).getByTestId("sidebar-more");
    await waitFor(() => {
      expect(within(more).getByTestId("more-attention").textContent).toBe("1");
    });
    expect(more.getAttribute("aria-label")).toBe("More, Activity, 1 need attention, 0 waiting");
    fireEvent.click(screen.getByTestId("sidebar-section-views"));
    expect(within(screen.getByTestId("sidebar-section-views")).getByTestId("section-attention").textContent).toBe("1");
  });
});
