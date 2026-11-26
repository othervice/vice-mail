// Stage 22 (decision 0077): the learning spam filter in the UI: the setting in Settings → Inbox
// (off by default, "runs only while the app runs", learning from existing mail with progress and
// "Stop", accounts left out, "Forget" after a question), the marks on list rows, the note in the
// reading pane with "Not spam" and "Move to Junk", and the notice when it moved new mail.
import { act, screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Language } from "../src/i18n";
import type { MessageListItem, SpamFilterStatus } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { ipcError, makeAccount, makeMailbox, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { focusRingReport } from "./support/focusRing";
import { optionName } from "./support/names";
import { renderApp } from "./support/render";
import { SPAM_OFF } from "./support/spam";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

const ADA = makeAccount({ writesEnabled: true });
const BOB = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "bob@vicemail.test", writesEnabled: false });
const JUNK = makeMailbox({ id: 9, accountId: ADA.id, path: "Junk", role: "junk", roleSource: "server" });

const ON: SpamFilterStatus = {
  ...SPAM_OFF,
  settings: { enabled: true, excludedAccounts: [] },
  learnedSpam: 12,
  learnedHam: 340,
  tokens: 9000,
};

function openSettings(
  language: Language = "en",
  status: Partial<SpamFilterStatus> = {},
  failures: NonNullable<Parameters<typeof mockCore>[0]>["failures"] = {},
) {
  const core = mockCore({
    accounts: [ADA, BOB],
    mailboxes: standardMailboxes(),
    preferences: { theme: "system", language },
    spam: { status },
    failures,
  });
  const rendered = renderApp(language);
  act(() => {
    useUiStore.getState().showSettings("inbox");
  });
  return { core, ...rendered };
}

describe("spam filter: settings", () => {
  it("is off by default and says that it runs only while the app runs", async () => {
    const { core, user } = openSettings();
    const toggle = await screen.findByRole("switch", { name: "Learning spam filter" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    const section = screen.getByTestId("spam-settings");
    expect(section.textContent).toContain("runs only while the app runs");
    expect(section.textContent).toContain("stays on this device");
    // Nothing else while it is off.
    expect(within(section).queryByTestId("spam-learn")).toBeNull();

    await user.click(toggle);
    await waitFor(() => {
      expect(core.callsOf("set_spam_filter")).toEqual([{ settings: { enabled: true, excludedAccounts: [] } }]);
    });
    await waitFor(() => {
      expect(toggle.getAttribute("aria-checked")).toBe("true");
    });
    expect(screen.getByTestId("spam-learned").textContent).toContain("Learned so far: 0 spam and 0 other mails.");
    expect(screen.getByTestId("spam-ready").textContent).toContain("at least 50 spam and 200 other mails");
  });

  it("learns from existing mail with progress, and stops", async () => {
    const { core, user } = openSettings("en", ON);
    await user.click(await screen.findByTestId("spam-learn"));
    await waitFor(() => {
      expect(core.callsOf("learn_spam_from_mail")).toHaveLength(1);
    });
    const state = await screen.findByTestId("spam-learning-state");
    expect(state.getAttribute("role")).toBe("status");
    expect(state.textContent).toContain("Learning… 0 of 1,500 mails");
    expect(within(state).getByRole("progressbar", { name: "Learn from existing mail" })).toBeTruthy();

    // Progress comes from the core.
    await emitCore({
      type: "spamFilterChanged",
      status: { ...ON, learning: { state: "running", done: 600, total: 1500, spam: 300, ham: 280, skipped: 20, error: null } },
    });
    await waitFor(() => {
      expect(screen.getByTestId("spam-learning-state").textContent).toContain("600 of 1,500");
    });
    expect(screen.getByTestId("spam-learning-state").textContent).toContain("20 mails left out.");

    await user.click(screen.getByTestId("spam-learning-cancel"));
    expect(core.callsOf("cancel_spam_learning")).toHaveLength(1);
    await emitCore({
      type: "spamFilterChanged",
      status: {
        ...ON,
        learnedSpam: 312,
        learnedHam: 620,
        ready: true,
        learning: { state: "cancelled", done: 600, total: 1500, spam: 300, ham: 280, skipped: 20, error: null },
      },
    });
    await waitFor(() => {
      expect(screen.getByTestId("spam-learning-state").getAttribute("data-state")).toBe("cancelled");
    });
    expect(screen.getByTestId("spam-learning-state").textContent).toContain("Stopped. Learned 300 spam and 280 other mails");
    expect(screen.getByTestId("spam-ready").textContent).toBe("It looks at new mail in your inboxes.");
    expect(screen.getByTestId("spam-learn")).toBeTruthy();
  });

  it("leaves an account out and says where it only marks", async () => {
    const { core, user } = openSettings("en", ON);
    const accounts = await screen.findByRole("group", { name: "Use for these accounts" });
    const ada = within(accounts).getByRole<HTMLInputElement>("checkbox", { name: /ada@vicemail\.test/ });
    const bob = within(accounts).getByRole("checkbox", { name: /bob@vicemail\.test/ });
    expect(ada.checked).toBe(true);
    expect(bob.closest("label")?.textContent).toContain("only marked on this device");
    await user.click(ada);
    await waitFor(() => {
      expect(core.callsOf("set_spam_filter")).toEqual([{ settings: { enabled: true, excludedAccounts: [ADA.id] } }]);
    });
    await waitFor(() => {
      expect(within(accounts).getByRole<HTMLInputElement>("checkbox", { name: /ada@vicemail\.test/ }).checked).toBe(false);
    });
  });

  it("forgets what it learned only after a question", async () => {
    const { core, user } = openSettings("en", ON);
    await user.click(await screen.findByTestId("spam-forget"));
    const dialog = await screen.findByRole("alertdialog", { name: "Forget what the spam filter learned?" });
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(core.callsOf("forget_spam_filter")).toEqual([]);
    await user.click(screen.getByTestId("spam-forget"));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Forget" }));
    await waitFor(() => {
      expect(core.callsOf("forget_spam_filter")).toHaveLength(1);
    });
    await waitFor(() => {
      expect(screen.getByTestId("spam-learned").textContent).toContain("0 spam and 0 other mails");
    });
  });

  it("shows a typed error when learning is refused", async () => {
    const { user } = openSettings("en", ON, { learn_spam_from_mail: ipcError("database") });
    await user.click(await screen.findByTestId("spam-learn"));
    const notice = await screen.findByText("Learning from your mail failed.");
    expect(notice.closest("[data-error-code]")?.getAttribute("data-error-code")).toBe("database");
  });

  it("speaks German (du)", async () => {
    openSettings("de", ON);
    expect(await screen.findByRole("switch", { name: "Lernender Spamfilter" })).toBeTruthy();
    await screen.findByTestId("spam-learned");
    const section = screen.getByTestId("spam-settings");
    expect(section.textContent).toContain("läuft nur, solange die App läuft");
    expect(section.textContent).toContain("Bisher gelernt: 12 Spam- und 340 andere Mails.");
    expect(within(section).getByRole("button", { name: "Jetzt lernen" })).toBeTruthy();
    expect(within(section).getByRole("group", { name: "Für diese Konten verwenden" })).toBeTruthy();
  });
});

// ---------------------------------------------------------------------------------------------
// Rows, the reading pane and the notice

interface ReadingSetup {
  account?: typeof ADA;
  mark: "suspected" | "moved";
  canMove?: boolean;
  language?: Language;
  theme?: "system" | "light" | "dark";
}

function openFolder({ account = ADA, mark, canMove = false, language = "en", theme = "system" }: ReadingSetup) {
  const mailboxId = mark === "moved" ? JUNK.id : 1;
  const items = makeMessages(2, { mailboxId });
  const first = { ...(items[0] as MessageListItem), accountId: account.id, spam: mark };
  const second = { ...(items[1] as MessageListItem), accountId: account.id };
  const core = mockCore({
    accounts: [account],
    mailboxes: [...standardMailboxes(account.id), { ...JUNK, accountId: account.id }],
    messages: { [mailboxId]: [first, second] },
    views: { [first.id]: makeView(first), [second.id]: makeView(second) },
    readingPreferences: { markReadOnOpen: "never" },
    preferences: { theme, language },
    spam: {
      status: ON,
      verdicts: { [first.id]: { mailboxId, mark, score: 0.9973, at: Date.now(), canMove } },
    },
  });
  const rendered = renderApp(language);
  if (mark === "moved") {
    act(() => {
      useUiStore.getState().showMailbox(account.id, JUNK.id);
    });
  }
  return { core, first, second, mailboxId, ...rendered };
}

async function openMessage(user: UserEvent, subject: string) {
  const listbox = await screen.findByRole("listbox", { name: /^(Messages|Nachrichten) in/ });
  const row = within(listbox)
    .getAllByRole("option")
    .find((o) => optionName(o).includes(subject));
  if (!row) throw new Error("no row");
  await user.click(row);
  await screen.findByTestId("message-card");
}

describe("spam filter: marks, note and notice", () => {
  it("marks a suspected row with a word that the row's name says too", async () => {
    const { first, second } = openFolder({ mark: "suspected" });
    const listbox = await screen.findByRole("listbox", { name: /^Messages in/ });
    const rows = within(listbox).getAllByRole("option");
    const suspected = rows.find((o) => optionName(o).includes(first.subject));
    const clean = rows.find((o) => optionName(o).includes(second.subject));
    expect(suspected && within(suspected).getByTestId("row-mark-spamSuspected").textContent).toBe("Spam?");
    expect(suspected && optionName(suspected)).toContain("Suspected spam (spam filter)");
    expect(clean && within(clean).queryByTestId("row-mark-spamSuspected")).toBeNull();
  });

  it("“Not spam” on a suspected mail of a read-only account removes the mark here, with undo", async () => {
    const { core, user, first } = openFolder({ account: BOB, mark: "suspected" });
    await openMessage(user, first.subject);
    const note = await screen.findByTestId("spam-note");
    expect(note.getAttribute("role")).toBe("note");
    expect(note.textContent).toContain("The spam filter thinks this mail is spam (99.7% sure).");
    expect(note.textContent).toContain("only marked on this device");
    expect(within(note).queryByTestId("spam-note-move")).toBeNull();
    await user.click(within(note).getByRole("button", { name: "Not spam" }));
    await waitFor(() => {
      expect(core.spam.local).toEqual([{ messageIds: [first.id], spam: false }]);
    });
    const notice = await screen.findByText("Marked as not spam");
    await waitFor(() => {
      expect(screen.queryByTestId("spam-note")).toBeNull();
    });
    // Undo marks it again.
    const undo = within(notice.closest("[data-testid='notice']") as HTMLElement).getByRole("button", { name: "Undo" });
    await user.click(undo);
    await waitFor(() => {
      expect(core.spam.local).toEqual([
        { messageIds: [first.id], spam: false },
        { messageIds: [first.id], spam: true },
      ]);
    });
    expect(core.callsOf("apply_action")).toEqual([]);
  });

  it("“Move to Junk” where the account may write and has a junk folder", async () => {
    const { core, user, first } = openFolder({ mark: "suspected", canMove: true });
    await openMessage(user, first.subject);
    const note = await screen.findByTestId("spam-note");
    await user.click(within(note).getByRole("button", { name: "Move to Junk" }));
    await waitFor(() => {
      expect(core.callsOf("apply_action")).toEqual([
        expect.objectContaining({ action: { type: "spam" }, messages: [{ mailboxId: 1, messageId: first.id }] }),
      ]);
    });
    expect(await screen.findByText("Moved to junk")).toBeTruthy();
  });

  it("“Not spam” in Junk moves a mail the filter moved back through the outbox", async () => {
    const { core, user, first } = openFolder({ mark: "moved" });
    const listbox = await screen.findByRole("listbox", { name: /^Messages in/ });
    const row = within(listbox)
      .getAllByRole("option")
      .find((o) => optionName(o).includes(first.subject));
    expect(row && within(row).getByTestId("row-mark-spamMoved").textContent).toBe("Spam filter");
    await openMessage(user, first.subject);
    const note = await screen.findByTestId("spam-note");
    expect(note.textContent).toContain("The spam filter moved this mail to Junk");
    await user.click(within(note).getByRole("button", { name: "Not spam" }));
    await waitFor(() => {
      expect(core.callsOf("apply_action")).toEqual([
        expect.objectContaining({ action: { type: "notSpam" }, messages: [{ mailboxId: JUNK.id, messageId: first.id }] }),
      ]);
    });
    expect(await screen.findByText("Moved to the inbox")).toBeTruthy();
  });

  it("tells when it moved new mail, and “Undo” undoes the batch", async () => {
    const { core, user } = openFolder({ mark: "suspected" });
    await screen.findByRole("listbox", { name: /^Messages in/ });
    await emitCore({ type: "spamFiltered", accountId: ADA.id, moved: 2, marked: 0, batch: 77 });
    const text = await screen.findByText("The spam filter moved 2 new mails to Junk");
    const notice = text.closest("[data-testid='notice']") as HTMLElement;
    expect(notice.textContent).toContain("teaches the filter");
    await user.click(within(notice).getByRole("button", { name: "Undo" }));
    await waitFor(() => {
      expect(core.callsOf("undo_action")).toEqual([{ batch: 77 }]);
    });
    // Marked only: no undo.
    await emitCore({ type: "spamFiltered", accountId: ADA.id, moved: 0, marked: 1, batch: null });
    const marked = await screen.findByText("The spam filter marked a new mail as suspected spam");
    expect(within(marked.closest("[data-testid='notice']") as HTMLElement).queryByRole("button", { name: "Undo" })).toBeNull();
  });

  it("speaks German in the note", async () => {
    const { user, first } = openFolder({ account: BOB, mark: "suspected", language: "de" });
    await openMessage(user, first.subject);
    const note = await screen.findByTestId("spam-note");
    expect(note.textContent).toContain("Der Spamfilter hält diese Mail für Spam (zu 99,7");
    expect(within(note).getByRole("button", { name: "Kein Spam" })).toBeTruthy();
  });
});

describe.each(["light", "dark"] as const)("spam filter, %s theme", (theme) => {
  it("settings have no serious axe findings and focus rings", async () => {
    mockCore({
      accounts: [ADA, BOB],
      mailboxes: standardMailboxes(),
      preferences: { theme, language: "system" },
      spam: { status: { ...ON, learning: { state: "running", done: 3, total: 10, spam: 2, ham: 1, skipped: 0, error: null } } },
    });
    renderApp();
    act(() => {
      useUiStore.getState().showSettings("inbox");
    });
    const section = await screen.findByTestId("spam-settings");
    const results = await axe.run(document.body, {
      resultTypes: ["violations"],
      iframes: false,
      rules: { "color-contrast": { enabled: false } },
    });
    expect(results.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => v.id)).toEqual([]);
    const report = await focusRingReport(section);
    expect(report.findings).toEqual([]);
    expect(report.checked).toBeGreaterThan(3);
  });

  it("the note has no serious axe findings", async () => {
    const { user, first } = openFolder({ mark: "suspected", canMove: true, theme });
    await openMessage(user, first.subject);
    const note = await screen.findByTestId("spam-note");
    const results = await axe.run(document.body, {
      resultTypes: ["violations"],
      iframes: false,
      rules: { "color-contrast": { enabled: false } },
    });
    expect(results.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => v.id)).toEqual([]);
    const report = await focusRingReport(note);
    expect(report.findings).toEqual([]);
  });
});
