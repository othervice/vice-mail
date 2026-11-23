// The reading pane's polish: the toolbar folds what does not fit into "More actions" (every action
// stays reachable), AI chips under the open message only where AI may work on it, and the ask bar
// that hands a question to the AI panel without sending anything before consent.
import { act, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fittingCount, visibleEntries } from "../src/features/actions/ActionButtons";
import type { ActionId } from "../src/features/actions/registry";
import type { MessageView } from "../src/services/ipc";
import { CLOUD_PROFILE_ID, makeAiProfile, makeCloudProfile, makeTransfer, type AiFakeOptions } from "./support/ai";
import { makeAccount, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";
import { DEFAULT_UI_PREFERENCES } from "./support/uiPrefs";

const ADA = makeAccount({ writesEnabled: true });
const LOCAL = makeAiProfile();
const ALL_LOCAL = { summary: LOCAL.id, reply_suggestion: LOCAL.id, writing_help: LOCAL.id, search: LOCAL.id };
const ALL_CLOUD = { summary: CLOUD_PROFILE_ID, reply_suggestion: CLOUD_PROFILE_ID, writing_help: CLOUD_PROFILE_ID, search: CLOUD_PROFILE_ID };
const FOLDED = { ui: { preferences: { ...DEFAULT_UI_PREFERENCES, aiPanel: "folded" as const } } };

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

function setup(ai: AiFakeOptions = { enabled: false }, extra: Parameters<typeof mockCore>[0] = {}, view: Partial<MessageView> = {}) {
  const message = makeMessage({
    id: 5001,
    subject: "Budget 2027",
    from: { name: "Grace Hopper", email: "grace@example.org" },
    date: new Date(2026, 10, 20, 14, 30).getTime(),
  });
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: [message] },
    views: { [message.id]: makeView(message, view) },
    readingPreferences: { markReadOnOpen: "never" },
    ai,
    ...extra,
  });
  return { core, message, ...renderApp() };
}

async function openMessage(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("option"));
  return screen.findByRole("region", { name: "Message" });
}

const nameOf = (element: HTMLElement) => element.getAttribute("aria-label") ?? element.textContent;

describe("reading toolbar", () => {
  it("counts how many buttons fit next to “More”, and all of them without a measurement", () => {
    expect(fittingCount(1000, 32, 4, 10)).toBeNull();
    // 7 buttons and their gaps (7 × 32 + 7 × 4 = 252) plus “More” (32) fit into 300 px.
    expect(fittingCount(300, 32, 4, 20)).toBe(7);
    expect(fittingCount(20, 32, 4, 20)).toBe(0);
    // A read-only note can take all the room: then only "More" is left, never everything.
    expect(fittingCount(-40, 32, 4, 20)).toBe(0);
    // Nothing measured (no button width): nothing is hidden.
    expect(fittingCount(300, 0, 4, 20)).toBeNull();
  });

  it("shows only the primary actions, and in a narrow pane the most used of them, in toolbar order", () => {
    const entries = (["reply", "replyAll", "forward", "archive", "delete", "toggleFlag", "move", "openInWindow"] as ActionId[]).map((id) => ({
      definition: { id },
    }));
    const ids = (list: { definition: { id: ActionId } }[]) => list.map((entry) => entry.definition.id);
    expect(ids(visibleEntries(entries))).toEqual(["reply", "replyAll", "forward", "archive", "delete", "toggleFlag"]);
    expect(ids(visibleEntries(entries, 3))).toEqual(["reply", "archive", "delete"]);
    expect(visibleEntries(entries, 0)).toEqual([]);
    expect(ids(visibleEntries(entries, 99))).toEqual(ids(visibleEntries(entries)));
  });

  it("groups respond, file away and mark with lines between them; everything else is in “More”", async () => {
    const { user } = setup();
    const pane = await openMessage(user);
    const toolbar = within(pane).getByRole("toolbar", { name: "Message actions" });
    expect(within(toolbar).getAllByRole("button").map(nameOf)).toEqual([
      "Reply",
      "Reply all",
      "Forward",
      "Archive",
      "Delete",
      "Mark as unread",
      "Flag",
      "More actions",
    ]);
    // Three thin lines: after Forward, after Delete, before "More" (decorative).
    const lines = toolbar.querySelectorAll("[data-toolbar-separator]");
    expect(lines).toHaveLength(3);
    for (const line of lines) expect(line.getAttribute("aria-hidden")).toBe("true");
    // The HTML/plain-text switch is not in the toolbar but at the body.
    expect(within(toolbar).queryByRole("group", { name: "Show as" })).toBeNull();
    expect(within(within(pane).getByTestId("message-card")).getByRole("group", { name: "Show as" })).toBeTruthy();
  });

  it("moves what does not fit into “More actions”, every action reachable by mouse and keyboard", async () => {
    // A narrow pane: 240 px for 32 px buttons and 1 px lines (no gaps in jsdom) leaves six of the
    // seven primary buttons next to “More”; Flag joins the menu.
    let observed = 0;
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {
          observed += 1;
        }
        unobserve() {}
        disconnect() {}
      },
    );
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(240);
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      const width = this.hasAttribute("data-toolbar-separator") ? 1 : 32;
      return { width, height: 32, top: 0, left: 0, right: width, bottom: 32, x: 0, y: 0, toJSON: () => ({}) };
    });
    const { core, user } = setup();
    const pane = await openMessage(user);
    const toolbar = within(pane).getByRole("toolbar", { name: "Message actions" });
    const more = await within(toolbar).findByRole("button", { name: "More actions" });
    expect(observed).toBeGreaterThan(0);

    const all = within(toolbar)
      .getAllByRole("button", { hidden: true })
      .filter((button) => button !== more)
      .map(nameOf);
    const shown = within(toolbar)
      .getAllByRole("button")
      .filter((button) => button !== more)
      .map(nameOf);
    expect(shown).toEqual(["Reply", "Reply all", "Forward", "Archive", "Delete", "Mark as unread"]);

    // The menu holds exactly the rest, in toolbar order, with their shortcuts.
    await user.click(more);
    const menu = within(pane).getByRole("menu", { name: "More actions" });
    const items = within(menu).getAllByRole("menuitem");
    expect(items.map(nameOf)).toEqual(all.filter((name) => !shown.includes(name)));
    expect(items.map(nameOf)).toContain("Flag");
    expect(document.activeElement).toBe(items[0]);
    const flagItem = within(menu).getByRole("menuitem", { name: "Flag" });
    expect(flagItem.textContent).toContain("S");
    expect(flagItem.getAttribute("aria-keyshortcuts")).toBe("S");

    // Escape closes it and gives the focus back.
    await user.keyboard("{Escape}");
    expect(within(pane).queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(more);

    // From the toolbar's first button: End reaches “More”, Arrow Down opens it on the first item.
    const first = within(toolbar).getAllByRole("button")[0] as HTMLElement;
    first.focus();
    await user.keyboard("{End}");
    expect(document.activeElement).toBe(more);
    await user.keyboard("{ArrowDown}");
    const reopened = within(pane).getByRole("menu", { name: "More actions" });
    expect(document.activeElement).toBe(within(reopened).getAllByRole("menuitem")[0]);
    await user.keyboard("{ArrowUp}");
    const flag = within(reopened).getByRole("menuitem", { name: "Flag" });
    flag.focus();
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(core.callsOf("apply_action")).toHaveLength(1);
    });
    expect(within(pane).queryByRole("menu")).toBeNull();
  });
});

describe("reading toolbar in a very narrow pane", () => {
  it("keeps Reply folded away when it measures again, instead of flipping back to every button", async () => {
    // 100 px leave room for Archive and Delete next to “More”: Reply, the first button in the
    // toolbar, is folded away. A folded (hidden) button measures 0 like in a browser; measuring
    // with it read as “nothing measured” and showed every button again on the next callback.
    const callbacks: (() => void)[] = [];
    class ManualResizeObserver implements ResizeObserver {
      constructor(callback: ResizeObserverCallback) {
        callbacks.push(() => {
          callback([], this);
        });
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal("ResizeObserver", ManualResizeObserver);
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(100);
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      const width = this.hasAttribute("data-toolbar-separator") ? 1 : this.closest("[hidden]") ? 0 : 32;
      return { width, height: 32, top: 0, left: 0, right: width, bottom: 32, x: 0, y: 0, toJSON: () => ({}) };
    });
    const { user } = setup();
    const pane = await openMessage(user);
    const toolbar = within(pane).getByRole("toolbar", { name: "Message actions" });
    const shown = () => within(toolbar).getAllByRole("button").map(nameOf);
    await waitFor(() => {
      expect(shown()).toEqual(["Archive", "Delete", "More actions"]);
    });
    // The pane or the read-only note changes size: the toolbar measures again, twice.
    for (let round = 0; round < 2; round += 1) {
      act(() => {
        for (const callback of callbacks) callback();
      });
      expect(shown()).toEqual(["Archive", "Delete", "More actions"]);
    }
  });
});

describe("the open message", () => {
  it("is a card under its subject with a reply row; with AI off there are no AI chips and no ask bar", async () => {
    const { user } = setup({ enabled: false });
    const pane = await openMessage(user);
    expect(within(pane).getByRole("heading", { level: 2, name: "Budget 2027" })).toBeTruthy();
    const card = within(pane).getByTestId("message-card");
    const row = within(card).getByRole("group", { name: "Reply and forward" });
    expect(within(row).getAllByRole("button").map(nameOf)).toEqual(["Reply to Grace Hopper", "Reply to everyone", "Forward this message"]);
    expect(within(card).getByRole("button", { name: "More for this message" })).toBeTruthy();
    expect(within(pane).queryByTestId("quick-replies")).toBeNull();
    expect(within(pane).queryByTestId("ask-bar")).toBeNull();
  });

  it("folds recipients and account into one line that Enter unfolds; the sender's address always shows", async () => {
    const { user } = setup({ enabled: false }, {}, {
      header: {
        from: [{ name: "Grace Hopper", email: "grace@example.org" }],
        to: [{ name: "Ada Lovelace", email: ADA.email }, { name: "Ethan Bennett", email: "ethan@example.org" }],
        cc: [{ name: "Mira Okafor", email: "mira@example.org" }],
        bcc: [],
        replyTo: [{ name: null, email: "billing@elsewhere.example" }],
        date: new Date(2026, 10, 20, 14, 30).getTime(),
        subject: "Budget 2027",
      },
    });
    const pane = await openMessage(user);
    expect(within(pane).getByTestId("reading-from").textContent).toBe("From: Grace Hopper grace@example.org");
    const summary = within(pane).getByRole("button", { name: /^to you, Ethan Bennett \+1/ });
    // A reply address other than the sender's is a sign of a forged mail: it stays in the line.
    expect(summary.textContent).toContain("replies go to billing@elsewhere.example");
    expect(summary.getAttribute("aria-expanded")).toBe("false");
    expect(within(pane).queryByTestId("reading-details")).toBeNull();

    summary.focus();
    await user.keyboard("{Enter}");
    expect(summary.getAttribute("aria-expanded")).toBe("true");
    const details = within(pane).getByTestId("reading-details");
    expect(summary.getAttribute("aria-controls")).toBe(details.id);
    expect(within(details).getByText("Mira Okafor")).toBeTruthy();
    expect(within(details).getByTestId("reading-account").textContent).toBe("Ada Lovelace (ada@vicemail.test)");
    await user.keyboard("{Enter}");
    expect(within(pane).queryByTestId("reading-details")).toBeNull();
  });

  it("shows the category as a quiet chip whose “Why here?” works from the keyboard", async () => {
    const { user } = setup();
    const pane = await openMessage(user);
    const why = await within(pane).findByRole("button", { name: "Why here?" });
    expect(why.getAttribute("aria-expanded")).toBe("false");
    why.focus();
    await user.keyboard("{Enter}");
    expect(why.getAttribute("aria-expanded")).toBe("true");
    expect(await within(pane).findByTestId("category-details")).toBeTruthy();
  });

  it("offers no AI chips for an account a cloud provider may not see", async () => {
    const { user } = setup({ enabled: true, profiles: [makeCloudProfile()], functions: ALL_CLOUD, cloudAccounts: [] });
    const pane = await openMessage(user);
    const panel = await screen.findByRole("complementary", { name: "AI assistant" });
    await within(panel).findByTestId("ai-not-permitted");
    expect(within(pane).queryByTestId("quick-replies")).toBeNull();
  });

  it("a chip asks the panel for a reply suggestion: an editable card, no draft before “Open draft”", async () => {
    const { core, user, message } = setup(
      {
        enabled: true,
        profiles: [LOCAL],
        functions: ALL_LOCAL,
        reply: (messageId) => ({ inReplyTo: messageId, text: "Hi Grace, Friday works.", transfer: makeTransfer() }),
      },
      FOLDED,
    );
    const pane = await openMessage(user);
    expect(screen.queryByRole("complementary", { name: "AI assistant" })).toBeNull();
    const chips = await within(pane).findByRole("group", { name: "AI for this message" });
    await user.click(within(chips).getByRole("button", { name: "Suggest reply" }));

    const panel = await screen.findByRole("complementary", { name: "AI assistant" });
    const card = await within(panel).findByRole("article", { name: "Reply suggestion" });
    expect(within(card).getByRole<HTMLTextAreaElement>("textbox").value).toBe("Hi Grace, Friday works.");
    expect(core.callsOf("ai_suggest_reply")).toEqual([expect.objectContaining({ messageId: message.id })]);
    expect(core.callsOf("ai_open_reply_draft")).toEqual([]);
    expect(core.callsOf("create_draft")).toEqual([]);
  });
});

describe("the ask bar", () => {
  const cloud: AiFakeOptions = {
    enabled: true,
    profiles: [makeCloudProfile()],
    functions: ALL_CLOUD,
    cloudAccounts: [ADA.id],
    answer: () => ({ found: false, statements: [], query: "x", candidates: 0, excluded: 0, dropped: 0, transfers: [] }),
  };

  it("hands the question to the panel, which asks for consent before anything is sent", async () => {
    const { core, user } = setup(cloud, FOLDED);
    const pane = await openMessage(user);
    const input = await within(pane).findByRole("textbox", { name: "Ask the AI assistant" });
    await user.type(input, "When is the meeting?{Enter}");

    const panel = await screen.findByRole("complementary", { name: "AI assistant" });
    const consent = await within(panel).findByRole("group", { name: "Before the first request to OpenAI" });
    expect(consent).toBeTruthy();
    expect(within(panel).getByRole<HTMLTextAreaElement>("textbox", { name: "Question" }).value).toBe("When is the meeting?");
    expect(core.callsOf("ai_ask")).toEqual([]);
    expect(core.callsOf("set_ai_consent")).toEqual([]);
    // The panel is open now: the bar steps back.
    expect(within(pane).queryByTestId("ask-bar")).toBeNull();

    await user.click(within(consent).getByRole("button", { name: "Agree and send" }));
    await waitFor(() => {
      expect(core.callsOf("ai_ask")).toEqual([expect.objectContaining({ question: "When is the meeting?" })]);
    });
  });

  it("is not there while the panel is open, and its chips run the panel's own functions", async () => {
    const open = setup(cloud);
    const pane = await openMessage(open.user);
    await screen.findByRole("complementary", { name: "AI assistant" });
    expect(within(pane).queryByTestId("ask-bar")).toBeNull();
    open.unmount();

    const { core, user } = setup({ ...cloud, profiles: [LOCAL], functions: ALL_LOCAL, summary: () => ({ keyPoints: [], openQuestions: [], deadlines: [], dropped: 0, transfer: makeTransfer() }) }, FOLDED);
    const folded = await openMessage(user);
    const input = await within(folded).findByRole("textbox", { name: "Ask the AI assistant" });
    // Nothing to send yet: the button waits for a question.
    expect(within(folded).getByRole("button", { name: "Ask in the AI assistant" }).hasAttribute("disabled")).toBe(true);
    await user.click(input);
    const suggestions = within(folded).getByRole("group", { name: "Suggestions for the AI assistant" });
    await user.click(within(suggestions).getByRole("button", { name: "Summarise this conversation" }));
    const panel = await screen.findByRole("complementary", { name: "AI assistant" });
    await within(panel).findByRole("article", { name: "Summary" });
    expect(core.callsOf("ai_summarize_thread")).toHaveLength(1);
  });
});
