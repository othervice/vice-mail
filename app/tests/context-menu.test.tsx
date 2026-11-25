import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { Archive, Reply, Trash2 } from "lucide-react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ContextMenu, placeMenu, type MenuCloseReason, type MenuEntry } from "../src/components/ContextMenu";
import type { MailAction, MessageListItem, ThemePreference } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { makeAccount, makeMailbox, makeMessage, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { focusRingReport } from "./support/focusRing";
import { optionName } from "./support/names";
import { renderApp } from "./support/render";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  // jsdom has no canvas; axe only uses one for icon fonts, which the app does not have.
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

/** axe-core as in tests/a11y.test.tsx (colour contrast: tests/contrast.test.ts). */
async function seriousFindings(): Promise<string[]> {
  const results = await axe.run(document.body, { resultTypes: ["violations"], iframes: false, rules: { "color-contrast": { enabled: false } } });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}

describe("where a context menu goes", () => {
  const view = { width: 1000, height: 700 };
  const size = { width: 240, height: 300 };

  it("opens right of and below the pointer where it fits", () => {
    expect(placeMenu({ kind: "point", x: 100, y: 100 }, size, view)).toEqual({ x: 100, y: 100 });
  });

  it("flips to the left near the right edge and above near the bottom", () => {
    expect(placeMenu({ kind: "point", x: 900, y: 600 }, size, view)).toEqual({ x: 660, y: 300 });
  });

  it("is pushed in from the edge where neither side fits, and starts at the margin when taller than the window", () => {
    expect(placeMenu({ kind: "point", x: 100, y: 200 }, { width: 240, height: 650 }, view)).toEqual({ x: 100, y: 46 });
    expect(placeMenu({ kind: "point", x: 100, y: 200 }, { width: 240, height: 900 }, view).y).toBe(4);
    expect(placeMenu({ kind: "point", x: 990, y: 10 }, { width: 1200, height: 100 }, view).x).toBe(4);
  });

  it("puts a submenu beside its item, on the left where the right has no room", () => {
    expect(placeMenu({ kind: "side", left: 100, right: 340, top: 120 }, size, view)).toEqual({ x: 340, y: 120 });
    expect(placeMenu({ kind: "side", left: 600, right: 840, top: 120 }, size, view)).toEqual({ x: 360, y: 120 });
    expect(placeMenu({ kind: "side", left: 100, right: 340, top: 600 }, size, view)).toEqual({ x: 340, y: 396 });
  });
});

describe("the context menu component", () => {
  function renderMenu(entries?: MenuEntry[]) {
    const onClose = vi.fn<(reason: MenuCloseReason) => void>();
    const chosen: string[] = [];
    const item = (id: string, label: string, extra: object = {}): MenuEntry => ({
      kind: "item",
      id,
      label,
      onSelect: () => chosen.push(id),
      testId: id,
      ...extra,
    });
    render(
      <ContextMenu
        label="Actions"
        point={{ x: 10, y: 10 }}
        iconRow={{
          label: "Frequent",
          items: [
            { kind: "item", id: "reply", label: "Reply", icon: Reply, onSelect: () => chosen.push("reply") },
            { kind: "item", id: "archive", label: "Archive", icon: Archive, onSelect: () => chosen.push("archive") },
            { kind: "item", id: "delete", label: "Delete", icon: Trash2, disabled: true, describedBy: "why", onSelect: () => chosen.push("delete") },
          ],
        }}
        entries={
          entries ?? [
            item("read", "Mark as read"),
            { kind: "submenu", id: "move", label: "Move to", testId: "move", entries: [item("inbox", "Inbox"), item("projects", "Projects", { indent: 1 })] },
            { kind: "separator", id: "s" },
            item("pin", "Pin"),
            item("print", "Print", { disabled: true, describedBy: "why", title: "Print: not here" }),
            item("allow", "Allow changes", { note: { id: "why", text: "Read-only" } }),
          ]
        }
        onClose={onClose}
        data-testid="menu"
      />,
    );
    return { onClose, chosen, user: userEvent.setup() };
  }

  const focused = () => document.activeElement?.getAttribute("data-menu-entry");

  it("is a menu with a group of icons, takes the focus on its first item, and moves by rows", async () => {
    const { user } = renderMenu();
    const menu = screen.getByRole("menu", { name: "Actions" });
    expect(within(menu).getByRole("group", { name: "Frequent" })).toBeTruthy();
    await waitFor(() => {
      expect(focused()).toBe("reply");
    });
    // ← → move within the icons; ↓ leaves the row, ↑ comes back to it.
    await user.keyboard("{ArrowRight}");
    expect(focused()).toBe("archive");
    await user.keyboard("{ArrowLeft}");
    expect(focused()).toBe("reply");
    await user.keyboard("{ArrowDown}");
    expect(focused()).toBe("read");
    await user.keyboard("{ArrowDown}{ArrowDown}");
    expect(focused()).toBe("pin");
    await user.keyboard("{ArrowUp}{ArrowUp}{ArrowUp}");
    expect(focused()).toBe("reply");
    await user.keyboard("{End}");
    expect(focused()).toBe("allow");
    await user.keyboard("{Home}");
    expect(focused()).toBe("reply");
    // Type-ahead: the next item starting with the letter.
    await user.keyboard("p");
    expect(focused()).toBe("pin");
    await user.keyboard("p");
    expect(focused()).toBe("print");
  });

  it("opens a submenu with → on its first item, and goes back with ← or Escape", async () => {
    const { user, chosen, onClose } = renderMenu();
    await waitFor(() => {
      expect(focused()).toBe("reply");
    });
    await user.keyboard("{ArrowDown}{ArrowDown}");
    const trigger = screen.getByTestId("move");
    expect(document.activeElement).toBe(trigger);
    expect(trigger.getAttribute("aria-haspopup")).toBe("menu");
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    await user.keyboard("{ArrowRight}");
    const sub = await screen.findByRole("menu", { name: "Move to" });
    expect(trigger.getAttribute("aria-expanded")).toBe("true");
    expect(trigger.getAttribute("aria-controls")).toBe(sub.id);
    await waitFor(() => {
      expect(focused()).toBe("inbox");
    });
    // Subfolders start further in.
    expect(screen.getByTestId("projects").className).toContain("ps-6");
    await user.keyboard("{ArrowLeft}");
    expect(screen.queryByRole("menu", { name: "Move to" })).toBeNull();
    expect(document.activeElement).toBe(trigger);
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(focused()).toBe("inbox");
    });
    await user.keyboard("{Escape}");
    expect(document.activeElement).toBe(trigger);
    expect(onClose).not.toHaveBeenCalled();
    // Again, and choose: the menu closes before the item runs.
    await user.keyboard("{ArrowRight}{ArrowDown}{Enter}");
    expect(chosen).toEqual(["projects"]);
    expect(onClose).toHaveBeenCalledWith("select");
  });

  it("keeps disabled items focusable, says why, and does nothing when they are chosen", async () => {
    const { user, chosen, onClose } = renderMenu();
    const print = screen.getByTestId("print");
    expect(print.getAttribute("aria-disabled")).toBe("true");
    expect(print.getAttribute("aria-describedby")).toBe("why");
    expect(print.getAttribute("title")).toBe("Print: not here");
    expect(document.getElementById("why")?.textContent).toBe("Read-only");
    // The note describes the item that carries it and is no part of its name.
    expect(screen.getByRole("menuitem", { name: "Allow changes" })).toBeTruthy();
    await user.click(print);
    expect(chosen).toEqual([]);
    expect(onClose).not.toHaveBeenCalled();
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(chosen).toEqual([]);
  });

  it("closes with Escape, Tab and a press outside; a hovered submenu opens without taking the focus", async () => {
    const { user, onClose } = renderMenu();
    await waitFor(() => {
      expect(focused()).toBe("reply");
    });
    await user.hover(screen.getByTestId("move"));
    expect(await screen.findByRole("menu", { name: "Move to" })).toBeTruthy();
    expect(focused()).toBe("move");
    await user.hover(screen.getByTestId("pin"));
    expect(screen.queryByRole("menu", { name: "Move to" })).toBeNull();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenLastCalledWith("escape");
    await user.keyboard("{Tab}");
    expect(onClose).toHaveBeenLastCalledWith("tab");
    fireEvent.pointerDown(document.body);
    expect(onClose).toHaveBeenLastCalledWith("outside");
  });

  it("is placed through the CSSOM and stays inside the window", async () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ width: 240, height: 300, top: 0, left: 0, right: 240, bottom: 300, x: 0, y: 0, toJSON: () => ({}) });
    const width = vi.spyOn(window, "innerWidth", "get").mockReturnValue(1000);
    const height = vi.spyOn(window, "innerHeight", "get").mockReturnValue(700);
    render(<ContextMenu label="Edge" point={{ x: 950, y: 650 }} entries={[{ kind: "item", id: "a", label: "A", onSelect: () => undefined }]} onClose={() => undefined} />);
    const menu = screen.getByRole("menu", { name: "Edge" });
    await waitFor(() => {
      expect(menu.className).not.toContain("invisible");
    });
    expect(menu.style.getPropertyValue("--menu-x")).toBe("710px");
    expect(menu.style.getPropertyValue("--menu-y")).toBe("350px");
    expect(menu.getAttribute("style")).not.toBeNull();
    width.mockRestore();
    height.mockRestore();
  });
});

// ---- in the app ----

const ADA = makeAccount({ writesEnabled: true });

function mailboxes() {
  return [
    ...standardMailboxes(),
    makeMailbox({ id: 7, path: "Archive", role: "archive", roleSource: "server" }),
    makeMailbox({ id: 8, path: "Junk", role: "junk", roleSource: "server" }),
  ];
}

function setup({ account = ADA, items = makeMessages(6), theme = "light" }: { account?: typeof ADA; items?: MessageListItem[]; theme?: ThemePreference } = {}) {
  const core = mockCore({
    preferences: { theme, language: "en" },
    accounts: [account],
    mailboxes: mailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
  });
  return { core, items, ...renderApp() };
}

const actionCalls = (core: ReturnType<typeof mockCore>) => core.callsOf("apply_action") as { action: MailAction; messages: { mailboxId: number; messageId: number }[] }[];

async function list() {
  return screen.findByRole("listbox", { name: /^Messages in/ });
}

function row(listbox: HTMLElement, subject: string): HTMLElement {
  const option = within(listbox)
    .getAllByRole("option")
    .find((o) => optionName(o).includes(`, ${subject},`));
  if (!option) throw new Error(`no row ${subject}`);
  return option;
}

const selected = (listbox: HTMLElement) =>
  within(listbox)
    .getAllByRole("option")
    .filter((o) => o.getAttribute("aria-selected") === "true")
    .map((o) => /Message \d+(?=,)/.exec(optionName(o))?.[0] ?? "");

async function rightClick(element: HTMLElement, x = 200, y = 150) {
  await act(async () => {
    fireEvent.contextMenu(element, { clientX: x, clientY: y });
    await Promise.resolve();
  });
  return screen.findByRole("menu", { name: /^Actions for/ });
}

describe("the context menu of messages", () => {
  it("targets a row outside the selection alone, with a ring, and leaves the selection as it is", async () => {
    const { user } = setup();
    const listbox = await list();
    await waitFor(() => {
      expect(selected(listbox)).toEqual(["Message 1"]);
    });
    const third = row(listbox, "Message 3");
    const menu = await rightClick(third);
    expect(menu.getAttribute("aria-label")).toBe("Actions for this message");
    // The icons on top: Reply, Archive, Spam, Delete.
    const icons = within(menu).getByRole("group", { name: "Frequent actions" });
    expect(within(icons).getAllByRole("menuitem").map((item) => item.getAttribute("aria-label"))).toEqual(["Reply", "Archive", "Mark as junk", "Delete"]);
    expect(third.querySelector("[data-context-target]")).toBeTruthy();
    expect(selected(listbox)).toEqual(["Message 1"]);
    // The entries of the plan, in their order (what applies to one read message in the inbox).
    const names = within(menu)
      .getAllByRole("menuitem")
      .filter((item) => !icons.contains(item))
      // The visible label (the shortcut beside it is hidden from the name).
      .map((item) => item.querySelector("span:not([aria-hidden])")?.textContent);
    expect(names).toEqual([
      "Open",
      "Reply and forward",
      "Mark as unread",
      "Mark",
      "Pin",
      "Snooze",
      "Mark as task",
      "Add to reading list",
      "Move to",
      "Copy to",
      "Tags",
      "Edit as new",
      "View source",
      "Save as .eml file…",
      "Print…",
    ]);
    // Escape closes it; the list has the focus again, nothing else changed.
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu", { name: /^Actions for/ })).toBeNull();
    expect(document.activeElement).toBe(listbox);
    expect(third.querySelector("[data-context-target]")).toBeNull();
    expect(selected(listbox)).toEqual(["Message 1"]);
  });

  it("acts on the whole selection when the row belongs to it", async () => {
    const { core, user, items } = setup();
    const listbox = await list();
    await user.click(row(listbox, "Message 2"));
    // The selection box in the avatar's place adds a row (stage 21).
    await user.click(within(row(listbox, "Message 4")).getByTestId("row-check"));
    await waitFor(() => {
      expect(selected(listbox)).toEqual(["Message 2", "Message 4"]);
    });
    const menu = await rightClick(row(listbox, "Message 4"));
    expect(menu.getAttribute("aria-label")).toBe("Actions for 2 messages");
    // One message only: no Reply (disabled in the icon row), no Open, no Print.
    expect(within(menu).getByRole("menuitem", { name: "Reply" }).getAttribute("aria-disabled")).toBe("true");
    expect(within(menu).queryByRole("menuitem", { name: "Open" })).toBeNull();
    expect(within(menu).queryByRole("menuitem", { name: "Print…" })).toBeNull();
    await user.click(within(menu).getByRole("menuitem", { name: "Archive" }));
    await waitFor(() => {
      expect(actionCalls(core)).toEqual([
        expect.objectContaining({
          action: { type: "archive" },
          messages: [items[1], items[3]].map((item) => ({ mailboxId: 1, messageId: item?.id })),
        }),
      ]);
    });
    expect((await screen.findByTestId("notice")).textContent).toContain("2 messages archived");
  });

  it("opens from the keyboard (Shift+F10) on the cursor row; submenus by keyboard; Delete goes to the trash with undo", async () => {
    const { core, user, items } = setup();
    const listbox = await list();
    act(() => {
      listbox.focus();
    });
    await user.keyboard("{ArrowDown}");
    await waitFor(() => {
      expect(optionName(document.getElementById(listbox.getAttribute("aria-activedescendant") ?? "") as HTMLElement)).toContain("Message 2");
    });
    await user.keyboard("{Shift>}{F10}{/Shift}");
    const menu = await screen.findByRole("menu", { name: "Actions for this message" });
    await waitFor(() => {
      expect(document.activeElement?.getAttribute("aria-label")).toBe("Reply");
    });
    // Down to "Move to", → opens it on "Choose a folder…", ↓ to the first folder.
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement?.textContent).toBe("Open");
    await user.keyboard("{ArrowRight}");
    const open = await screen.findByRole("menu", { name: "Open" });
    await waitFor(() => {
      expect(within(open).getAllByRole("menuitem")[0]).toBe(document.activeElement);
    });
    await user.keyboard("{ArrowLeft}");
    expect(screen.queryByRole("menu", { name: "Open" })).toBeNull();
    // The icon row's Delete, from the keyboard.
    await user.keyboard("{Home}{ArrowRight}{ArrowRight}{ArrowRight}");
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Delete");
    await user.keyboard("{Enter}");
    expect(menu.isConnected).toBe(false);
    await waitFor(() => {
      expect(actionCalls(core)).toEqual([expect.objectContaining({ action: { type: "delete" }, messages: [{ mailboxId: 1, messageId: items[1]?.id }] })]);
    });
    const notice = await screen.findByTestId("notice");
    expect(notice.textContent).toContain("Moved to the trash");
    expect(within(notice).getByRole("button", { name: /Undo/ })).toBeTruthy();
  });

  it("moves into a folder of the submenu, and copies into another folder of the account", async () => {
    const { core, user, items } = setup();
    const listbox = await list();
    let menu = await rightClick(row(listbox, "Message 5"));
    await user.click(within(menu).getByRole("menuitem", { name: "Move to" }));
    const move = await screen.findByRole("menu", { name: "Move to" });
    // The folder it lies in is not offered; subfolders come after their parent, further in.
    expect(within(move).queryByRole("menuitem", { name: "Inbox" })).toBeNull();
    expect(within(move).getByRole("menuitem", { name: "2026" }).className).toContain("ps-6");
    await user.click(within(move).getByRole("menuitem", { name: "Projekte" }));
    await waitFor(() => {
      expect(actionCalls(core)).toEqual([expect.objectContaining({ action: { type: "move", target: 5 }, messages: [{ mailboxId: 1, messageId: items[4]?.id }] })]);
    });

    menu = await rightClick(row(listbox, "Message 6"));
    await user.click(within(menu).getByRole("menuitem", { name: "Copy to" }));
    const copy = await screen.findByRole("menu", { name: "Copy to" });
    await user.click(within(copy).getByRole("menuitem", { name: "Archive" }));
    await waitFor(() => {
      expect(core.parity.transfers).toEqual([expect.objectContaining({ messages: [{ mailboxId: 1, messageId: items[5]?.id }], targetMailboxId: 7, mode: "copy" })]);
    });
  });

  it("toggles a tag of the view, and saves the message as an .eml file", async () => {
    const items = makeMessages(4).map((item, index) => (index === 0 ? { ...item, keywords: ["Projekt"] } : item));
    const { core, user } = setup({ items });
    const listbox = await list();
    let menu = await rightClick(row(listbox, "Message 2"));
    await user.click(within(menu).getByRole("menuitem", { name: "Tags" }));
    const tags = await screen.findByRole("menu", { name: "Tags" });
    const tag = within(tags).getByRole("menuitemcheckbox", { name: "Projekt" });
    expect(tag.getAttribute("aria-checked")).toBe("false");
    await user.click(tag);
    await waitFor(() => {
      expect(actionCalls(core)).toEqual([expect.objectContaining({ action: { type: "addKeyword", keyword: "Projekt" }, messages: [{ mailboxId: 1, messageId: items[1]?.id }] })]);
    });

    menu = await rightClick(row(listbox, "Message 3"));
    await user.click(within(menu).getByRole("menuitem", { name: "Save as .eml file…" }));
    await waitFor(() => {
      expect(core.extras.state.exports).toEqual([items[2]?.id]);
    });
    await waitFor(() => {
      expect(screen.getAllByTestId("notice").some((notice) => notice.textContent.includes("Saved as /Users/ada/Documents/Hello.eml"))).toBe(true);
    });
  });

  it("prints a message by opening it in the reading pane first", async () => {
    const { core, user } = setup();
    const listbox = await list();
    const menu = await rightClick(row(listbox, "Message 4"));
    await user.click(within(menu).getByRole("menuitem", { name: "Print…" }));
    await waitFor(() => {
      expect(selected(listbox)).toEqual(["Message 4"]);
    });
    await waitFor(() => {
      expect(core.extras.state.prints).toBe(1);
    });
  });

  it("snoozes with a preset of its submenu", async () => {
    const { core, user, items } = setup();
    const listbox = await list();
    const menu = await rightClick(row(listbox, "Message 3"));
    await user.click(within(menu).getByRole("menuitem", { name: "Snooze" }));
    const snooze = await screen.findByRole("menu", { name: "Snooze" });
    await user.click(await within(snooze).findByRole("menuitem", { name: /^Tomorrow/ }));
    await waitFor(() => {
      expect(core.callsOf("snooze_messages")).toEqual([expect.objectContaining({ messageIds: [items[2]?.id], choice: { kind: "tomorrow" } })]);
    });
  });

  it("on a read-only account shows what writes disabled, with the reason and the way to allow changes", async () => {
    const readOnly = makeAccount({ writesEnabled: false });
    const { core, user } = setup({ account: readOnly });
    const listbox = await list();
    const menu = await rightClick(row(listbox, "Message 2"));
    const note = within(menu).getByRole("menuitem", { name: "Allow changes…" });
    const noteId = note.getAttribute("aria-describedby") ?? "";
    expect(document.getElementById(noteId)?.textContent).toContain("Read-only");
    for (const name of ["Archive", "Delete", "Mark as junk"]) {
      const item = within(menu).getByRole("menuitem", { name });
      expect(item.getAttribute("aria-disabled")).toBe("true");
      expect(item.getAttribute("aria-describedby")).toBe(noteId);
      expect(item.getAttribute("title")).toBe(`${name}: changes are turned off for this account`);
    }
    // Submenus whose every entry writes are disabled as a whole.
    for (const name of ["Move to", "Tags", "Mark"]) {
      const trigger = within(menu).getByRole("menuitem", { name });
      expect(trigger.getAttribute("aria-disabled")).toBe("true");
      expect(trigger.getAttribute("aria-describedby")).toBe(noteId);
      expect(trigger.getAttribute("aria-haspopup")).toBeNull();
    }
    // Copying needs the target to write: no other account here, so nothing to copy to either.
    expect(within(menu).getByRole("menuitem", { name: "Copy to" }).getAttribute("aria-disabled")).toBe("true");
    // Answering writes a local draft (redirecting sends, it is disabled inside).
    expect(within(menu).getByRole("menuitem", { name: "Reply and forward" }).getAttribute("aria-haspopup")).toBe("menu");
    // Local marks still work.
    expect(within(menu).getByRole("menuitem", { name: "Pin" }).getAttribute("aria-disabled")).toBeNull();
    await user.click(within(menu).getByRole("menuitem", { name: "Archive" }));
    expect(screen.getByRole("menu", { name: /^Actions for/ })).toBeTruthy();
    expect(actionCalls(core)).toEqual([]);
    await user.click(note);
    await waitFor(() => {
      expect(useUiStore.getState().view).toEqual({ kind: "settings", section: "accounts" });
    });
  });

  it("shows the focus ring on every entry of the menu and its submenu", async () => {
    const { user } = setup();
    const listbox = await list();
    const menu = await rightClick(row(listbox, "Message 2"));
    await user.click(within(menu).getByRole("menuitem", { name: "Move to" }));
    await screen.findByRole("menu", { name: "Move to" });
    const report = await focusRingReport(document.querySelector("[data-context-menu]") as HTMLElement);
    expect(report.findings).toEqual([]);
    expect(report.checked).toBeGreaterThan(20);
  });

  for (const theme of ["light", "dark"] as const) {
    it(`finds nothing serious with axe, menu and submenu open (${theme})`, async () => {
      const { user } = setup({ theme, account: makeAccount({ writesEnabled: false }) });
      const listbox = await list();
      const menu = await rightClick(row(listbox, "Message 2"));
      // A submenu with a disabled entry and its reason (Redirect sends).
      await user.click(within(menu).getByRole("menuitem", { name: "Reply and forward" }));
      const sub = await screen.findByRole("menu", { name: "Reply and forward" });
      expect(within(sub).getByRole("menuitem", { name: "Redirect" }).getAttribute("aria-disabled")).toBe("true");
      expect(document.documentElement.dataset["theme"]).toBe(theme);
      expect(await seriousFindings()).toEqual([]);
    });
  }
});

describe("lists that mix accounts (stage 21, decision 0065)", () => {
  const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.com", displayName: "Work", color: 3, writesEnabled: true });
  const CLUB = makeAccount({ id: "44444444-4444-4444-8444-444444444444", email: "club@example.net", displayName: "Club", color: 5, writesEnabled: false });
  const now = Date.now();
  const WORK_MAIL: MessageListItem = makeMessage({ id: 2000, accountId: WORK.id, mailboxId: 20, subject: "Projekt", date: now - 60_000 });
  const CLUB_MAIL: MessageListItem = makeMessage({ id: 1000, accountId: CLUB.id, mailboxId: 30, subject: "Training", date: now - 120_000 });

  function unified() {
    const core = mockCore({
      accounts: [WORK, CLUB],
      mailboxes: [
        makeMailbox({ id: 20, accountId: WORK.id, path: "INBOX", role: "inbox", roleSource: "server" }),
        makeMailbox({ id: 21, accountId: WORK.id, path: "Archive", role: "archive", roleSource: "server" }),
        makeMailbox({ id: 22, accountId: WORK.id, path: "Trash", role: "trash", roleSource: "server" }),
        makeMailbox({ id: 30, accountId: CLUB.id, path: "INBOX", role: "inbox", roleSource: "server" }),
        makeMailbox({ id: 31, accountId: CLUB.id, path: "Archive", role: "archive", roleSource: "server" }),
      ],
      messages: { 20: [WORK_MAIL], 30: [CLUB_MAIL] },
      views: Object.fromEntries([WORK_MAIL, CLUB_MAIL].map((m) => [m.id, makeView(m)])),
      readingPreferences: { markReadOnOpen: "never" },
    });
    const rendered = renderApp();
    act(() => {
      useUiStore.getState().showUnifiedInbox({ kind: "all" });
    });
    return { core, ...rendered };
  }

  const rowOf = (listbox: HTMLElement, subject: string) =>
    within(listbox)
      .getAllByRole("option")
      .find((o) => optionName(o).includes(subject)) as HTMLElement;

  it("shows the quick actions in every row, acting with the row's own account", async () => {
    const { core, user } = unified();
    const listbox = await screen.findByRole("listbox", { name: "Messages in All inboxes" });
    await waitFor(() => {
      expect(within(listbox).getAllByRole("option")).toHaveLength(2);
    });
    const work = rowOf(listbox, "Projekt");
    const archive = await within(work).findByTestId("quick-archive");
    await user.click(archive);
    await waitFor(() => {
      expect(actionCalls(core)).toEqual([expect.objectContaining({ action: { type: "archive" }, messages: [{ mailboxId: 20, messageId: WORK_MAIL.id }] })]);
    });
    // The read-only account's row: disabled with the reason, nothing sent.
    const club = rowOf(listbox, "Training");
    const clubArchive = await within(club).findByTestId("quick-archive");
    expect(clubArchive.getAttribute("aria-disabled")).toBe("true");
    expect(clubArchive.getAttribute("title")).toBe("Archive: changes are turned off for this account");
    await user.click(clubArchive);
    expect(actionCalls(core)).toHaveLength(1);
    expect((await screen.findAllByTestId("notice")).some((notice) => notice.textContent.includes("Changes are turned off"))).toBe(true);
  });

  it("opens the context menu with the row's own account and folder", async () => {
    const { core, user } = unified();
    const listbox = await screen.findByRole("listbox", { name: "Messages in All inboxes" });
    await waitFor(() => {
      expect(within(listbox).getAllByRole("option")).toHaveLength(2);
    });
    let menu = await rightClick(rowOf(listbox, "Projekt"));
    await user.click(within(menu).getByRole("menuitem", { name: "Delete" }));
    await waitFor(() => {
      expect(actionCalls(core)).toEqual([expect.objectContaining({ action: { type: "delete" }, messages: [{ mailboxId: 20, messageId: WORK_MAIL.id }] })]);
    });
    menu = await rightClick(rowOf(listbox, "Training"));
    expect(within(menu).getByRole("menuitem", { name: "Delete" }).getAttribute("aria-disabled")).toBe("true");
    await user.click(within(menu).getByRole("menuitem", { name: "Move to" }));
    expect(screen.queryByRole("menu", { name: "Move to" })).toBeNull();
    // Copying into the other account needs only that one to write.
    await user.click(within(menu).getByRole("menuitem", { name: "Copy to" }));
    const copy = await screen.findByRole("menu", { name: "Copy to" });
    expect(within(copy).getByRole("menuitem", { name: "Archive" }).getAttribute("aria-disabled")).toBe("true");
    await user.click(within(copy).getByRole("menuitem", { name: "Work" }));
    const work = await screen.findByRole("menu", { name: "Work" });
    await user.click(within(work).getByRole("menuitem", { name: "Archive" }));
    await waitFor(() => {
      expect(core.parity.transfers).toEqual([expect.objectContaining({ messages: [{ mailboxId: 30, messageId: CLUB_MAIL.id }], targetMailboxId: 21, mode: "copy" })]);
    });
  });
});
