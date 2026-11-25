import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import axe from "axe-core";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DELETED_FOR_MS, HIDE_AFTER_MS, HOVER_STALE_MS, newer, PopupApp } from "../src/features/popup/PopupApp";
import { createI18n, type Language } from "../src/i18n";
import type { IpcError, NewMailCard, NewMailPopup, PopupAppearance } from "../src/services/ipc";

/*
 * Stage 18 (decision 0051): the page of the new-mail popup's window, against a fake of its own
 * commands (the only ones it may call): cards, "N more", hidden content, read-only accounts,
 * Read, Delete with Undo, hiding itself (not while the pointer is on it), events with revisions,
 * the window's height, theme and language from the core, and axe in both themes.
 */

const ACCOUNT = "6f9619ff-8b86-4d11-b42d-00c04fc964ff";
const NOW = new Date(2026, 10, 24, 15, 30).getTime();

function card(id: number, overrides: Partial<NewMailCard> = {}): NewMailCard {
  return {
    accountId: ACCOUNT,
    mailboxId: 1,
    messageId: id,
    date: NOW - id * 60_000,
    content: {
      senderName: `Sender ${String(id)}`,
      senderEmail: `s${String(id)}@example.org`,
      subject: `Subject ${String(id)}`,
      snippet: `Snippet of message ${String(id)} with enough words for two lines`,
      picture: null,
    },
    account: null,
    writesEnabled: true,
    deleted: false,
    ...overrides,
  };
}

const APPEARANCE: PopupAppearance = { theme: "light", language: "en", accent: "orange" };

interface Fake {
  calls: { cmd: string; args: Record<string, unknown> }[];
  popup: NewMailPopup;
}

function fake(initial: Partial<NewMailPopup>, failures: Record<string, IpcError> = {}): Fake {
  const state: Fake = {
    calls: [],
    popup: { cards: [], more: 0, appearance: APPEARANCE, revision: 1, ...initial },
  };
  const change = (cards: NewMailCard[], more = state.popup.more) => {
    state.popup = { ...state.popup, cards, more, revision: state.popup.revision + 1 };
    return state.popup;
  };
  mockIPC(
    (cmd, raw) => {
      const args = (raw ?? {}) as Record<string, unknown>;
      state.calls.push({ cmd, args });
      const failure = failures[cmd];
      // Tauri rejects with the serialised error value, not an Error.
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
      if (failure) return Promise.reject(failure);
      const id = args["messageId"] as number | null | undefined;
      const cards = state.popup.cards;
      switch (cmd) {
        case "get_new_mail_popup":
          return state.popup;
        case "read_from_popup":
          return change(cards.filter((c) => c.messageId !== id));
        case "delete_from_popup":
          return change(cards.map((c) => (c.messageId === id ? { ...c, deleted: true } : c)));
        case "undo_popup_delete":
          return change(cards.map((c) => (c.messageId === id ? { ...c, deleted: false } : c)));
        case "dismiss_popup":
          return id === null ? change([], 0) : change(cards.filter((c) => c.messageId !== id));
        case "open_inbox_from_popup":
          return change([], 0);
        case "fit_new_mail_popup":
          return null;
        default:
          throw new Error(`the popup may not call ${cmd}`);
      }
    },
    { shouldMockEvents: true },
  );
  return state;
}

function renderPopup(language: Language = "en") {
  const i18n = createI18n(language);
  render(
    <I18nextProvider i18n={i18n}>
      <PopupApp />
    </I18nextProvider>,
  );
}

const cards = () => screen.queryAllByTestId("popup-card");

async function seriousFindings(): Promise<string[]> {
  const results = await axe.run(document.body, {
    resultTypes: ["violations"],
    rules: { "color-contrast": { enabled: false } },
  });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id}: ${violation.help} (${violation.nodes.map((n) => n.target.join(" ")).join(", ")})`);
}

beforeEach(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("the new-mail popup", () => {
  it("shows a card with picture or initials, sender, subject, snippet, time and both buttons", async () => {
    const f = fake({ cards: [card(1)] });
    renderPopup();
    const [only] = await screen.findAllByTestId("popup-card");
    if (!only) throw new Error("no card");
    const article = within(only);
    expect(screen.getByRole("main", { name: "New mail" })).toBeTruthy();
    expect(screen.getByRole("heading", { level: 1, name: "New mail" })).toBeTruthy();
    expect(screen.getByRole("article", { name: "Sender 1" })).toBe(only);
    expect(article.getByTestId("popup-subject").textContent).toBe("Subject 1");
    expect(article.getByTestId("popup-snippet").textContent).toContain("Snippet of message 1");
    expect(article.getByTestId("popup-avatar").textContent).toBe("S1");
    expect(only.querySelector("time")?.textContent).toMatch(/\d/);
    expect(article.getByRole("button", { name: "Read" })).toBeTruthy();
    expect(article.getByRole<HTMLButtonElement>("button", { name: "Delete" }).disabled).toBe(false);
    expect(article.queryByTestId("popup-read-only")).toBeNull();
    expect(article.queryByTestId("popup-account")).toBeNull();
    // A polite live region says what arrived; the window takes no focus.
    await waitFor(() => {
      expect(screen.getByRole("status").textContent).toBe("New message from Sender 1: Subject 1");
    });
    expect(document.activeElement).toBe(document.body);
    expect(f.calls.map((c) => c.cmd)).toContain("get_new_mail_popup");
  });

  it("shows the contact's picture where there is one, and the account with several accounts", async () => {
    const picture = "data:image/jpeg;base64,/9j/AA==";
    fake({
      cards: [
        card(1, {
          content: { senderName: null, senderEmail: "grace@example.org", subject: "", snippet: null, picture },
          account: { name: "Work", color: 2 },
        }),
      ],
    });
    renderPopup();
    const avatar = await screen.findByTestId("popup-avatar");
    expect(avatar.querySelector("img")?.getAttribute("src")).toBe(picture);
    expect(screen.getByTestId("popup-sender").textContent).toBe("grace@example.org");
    expect(screen.getByTestId("popup-subject").textContent).toBe("(no subject)");
    expect(screen.queryByTestId("popup-snippet")).toBeNull();
    expect(screen.getByTestId("popup-account").textContent).toBe("In Work");
  });

  it("disables Delete for a read-only account and says why", async () => {
    fake({ cards: [card(1, { writesEnabled: false })] });
    renderPopup();
    const remove = await screen.findByRole<HTMLButtonElement>("button", { name: "Delete" });
    expect(remove.disabled).toBe(true);
    const reason = screen.getByTestId("popup-read-only");
    expect(remove.getAttribute("aria-describedby")).toBe(reason.id);
    expect(reason.textContent).toMatch(/Read-only account/);
  });

  it("hides sender, subject and snippet when the content is hidden, and offers only Read", async () => {
    fake({ cards: [card(1, { content: null, account: { name: "Home", color: 0 } })] });
    renderPopup();
    const only = await screen.findByTestId("popup-card");
    expect(within(only).getByTestId("popup-sender").textContent).toBe("New message");
    expect(within(only).queryByTestId("popup-subject")).toBeNull();
    expect(within(only).queryByTestId("popup-snippet")).toBeNull();
    expect(within(only).queryByRole("button", { name: "Delete" })).toBeNull();
    expect(within(only).getByRole("button", { name: "Read" })).toBeTruthy();
    expect(within(only).getByTestId("popup-account").textContent).toBe("In Home");
    await waitFor(() => {
      expect(screen.getByRole("status").textContent).toBe("New message");
    });
    expect(document.body.textContent).not.toContain("Sender 1");
  });

  it("stacks three cards newest first and opens the inbox from “N more”", async () => {
    const f = fake({ cards: [card(1), card(2), card(3)], more: 2 });
    const user = userEvent.setup();
    renderPopup();
    await waitFor(() => {
      expect(cards()).toHaveLength(3);
    });
    expect(screen.getAllByTestId("popup-sender").map((e) => e.textContent)).toEqual(["Sender 1", "Sender 2", "Sender 3"]);
    await waitFor(() => {
      expect(screen.getByRole("status").textContent).toBe("5 new messages");
    });
    await user.click(screen.getByRole("button", { name: "2 more in the inbox" }));
    await waitFor(() => {
      expect(cards()).toHaveLength(0);
    });
    expect(f.calls.some((c) => c.cmd === "open_inbox_from_popup")).toBe(true);
  });

  it("Read asks the core for the message", async () => {
    const f = fake({ cards: [card(1), card(2)] });
    const user = userEvent.setup();
    renderPopup();
    const [first] = await screen.findAllByTestId("popup-card");
    if (!first) throw new Error("no card");
    await user.click(within(first).getByRole("button", { name: "Read" }));
    await waitFor(() => {
      expect(cards()).toHaveLength(1);
    });
    expect(f.calls.find((c) => c.cmd === "read_from_popup")?.args).toEqual({ messageId: 1 });
  });

  it("Delete shows Deleted with Undo; the card goes after a few seconds", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const f = fake({ cards: [card(1)] });
    const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
    renderPopup();
    await user.click(await screen.findByRole("button", { name: "Delete" }));
    await screen.findByTestId("popup-deleted");
    expect(screen.getByTestId("popup-deleted").textContent).toBe("Deleted · Subject 1");
    expect(within(screen.getByTestId("popup-card")).getByRole("button", { name: "Close" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Undo" }));
    await screen.findByRole("button", { name: "Delete" });
    expect(f.calls.filter((c) => c.cmd.includes("delete")).map((c) => [c.cmd, c.args])).toEqual([
      ["delete_from_popup", { messageId: 1 }],
      ["undo_popup_delete", { messageId: 1 }],
    ]);
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await screen.findByTestId("popup-deleted");
    // While the pointer is on the popup the card stays; it goes once the pointer left.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(DELETED_FOR_MS + 100);
    });
    expect(cards()).toHaveLength(1);
    // (The button under the pointer was replaced: point at the popup again, then leave it.)
    await user.hover(screen.getByTestId("popup-root"));
    await user.unhover(screen.getByTestId("popup-root"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(DELETED_FOR_MS + 100);
    });
    await waitFor(() => {
      expect(cards()).toHaveLength(0);
    });
    expect(f.calls.find((c) => c.cmd === "dismiss_popup")?.args).toEqual({ messageId: 1 });
  });

  it("says why an action failed, on the card", async () => {
    fake({ cards: [card(1)] }, { delete_from_popup: { code: "missing_mailbox", message: "no trash", detail: null } });
    const user = userEvent.setup();
    renderPopup();
    await user.click(await screen.findByRole("button", { name: "Delete" }));
    const alert = await screen.findByRole("alert");
    expect(alert.getAttribute("data-error-code")).toBe("missing_mailbox");
    expect(alert.textContent).toMatch(/^Didn’t work: /);
    expect(screen.getByRole<HTMLButtonElement>("button", { name: "Delete" }).disabled).toBe(false);
  });

  it("closes one card with its close button", async () => {
    const f = fake({ cards: [card(1), card(2)] });
    const user = userEvent.setup();
    renderPopup();
    const [first] = await screen.findAllByTestId("popup-card");
    if (!first) throw new Error("no card");
    await user.click(within(first).getByRole("button", { name: "Close" }));
    await waitFor(() => {
      expect(cards()).toHaveLength(1);
    });
    expect(f.calls.find((c) => c.cmd === "dismiss_popup")?.args).toEqual({ messageId: 1 });
  });

  it("hides itself after a while, but not while the pointer is on it", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const f = fake({ cards: [card(1)] });
    const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
    renderPopup();
    const root = await screen.findByTestId("popup-root");
    // The pointer rests on it: nothing happens.
    await user.hover(root);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(HIDE_AFTER_MS * 2);
    });
    expect(f.calls.some((c) => c.cmd === "dismiss_popup")).toBe(false);
    expect(cards()).toHaveLength(1);
    // It leaves: the full time again, then the popup asks to close every card.
    await user.unhover(root);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(HIDE_AFTER_MS - 500);
    });
    expect(f.calls.some((c) => c.cmd === "dismiss_popup")).toBe(false);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_000);
    });
    expect(f.calls.find((c) => c.cmd === "dismiss_popup")?.args).toEqual({ messageId: null });
    await waitFor(() => {
      expect(cards()).toHaveLength(0);
    });
  });

  it("waits while the page is hidden (lock screen, sleeping display) and hides itself once seen", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    const f = fake({ cards: [card(1)] });
    renderPopup();
    await screen.findByTestId("popup-card");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(HIDE_AFTER_MS * 3);
    });
    expect(f.calls.some((c) => c.cmd === "dismiss_popup")).toBe(false);
    visibility.mockReturnValue("visible");
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(HIDE_AFTER_MS - 500);
    });
    expect(f.calls.some((c) => c.cmd === "dismiss_popup")).toBe(false);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_000);
    });
    expect(f.calls.find((c) => c.cmd === "dismiss_popup")?.args).toEqual({ messageId: null });
  });

  it("counts a pointer that never said it left as gone after a minute without a sign of it", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const f = fake({ cards: [card(1)] });
    const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
    renderPopup();
    const root = await screen.findByTestId("popup-root");
    await user.hover(root);
    expect(root.hasAttribute("data-hovered")).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(HOVER_STALE_MS - 1_000);
    });
    expect(f.calls.some((c) => c.cmd === "dismiss_popup")).toBe(false);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_000);
    });
    expect(root.hasAttribute("data-hovered")).toBe(false);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(HIDE_AFTER_MS + 100);
    });
    expect(f.calls.find((c) => c.cmd === "dismiss_popup")?.args).toEqual({ messageId: null });
  });

  it("follows the core's changes and keeps the newest revision", async () => {
    fake({ cards: [card(1)], revision: 5 });
    renderPopup();
    await screen.findByTestId("popup-card");
    await act(async () => {
      await emit("new-mail-popup-changed", { cards: [card(9), card(1)], more: 0, appearance: APPEARANCE, revision: 6 });
    });
    await waitFor(() => {
      expect(cards()).toHaveLength(2);
    });
    await waitFor(() => {
      expect(screen.getByRole("status").textContent).toBe("New message from Sender 9: Subject 9");
    });
    // An older state that arrives late changes nothing.
    await act(async () => {
      await emit("new-mail-popup-changed", { cards: [], more: 0, appearance: APPEARANCE, revision: 4 });
    });
    expect(cards()).toHaveLength(2);
    const base: NewMailPopup = { cards: [], more: 0, appearance: APPEARANCE, revision: 3 };
    expect(newer(base, { ...base, revision: 2 }).revision).toBe(3);
    expect(newer(null, base).revision).toBe(3);
    expect(newer(base, { ...base, revision: 3, more: 1 }).more).toBe(1);
  });

  it("tells the shell its height so the window fits the cards", async () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 360, 264.4));
    const f = fake({ cards: [card(1), card(2)] });
    renderPopup();
    await screen.findAllByTestId("popup-card");
    await waitFor(() => {
      expect(f.calls.find((c) => c.cmd === "fit_new_mail_popup")?.args).toEqual({ height: 265 });
    });
  });

  it("takes theme, accent and language from the core", async () => {
    fake({ cards: [card(1)], appearance: { theme: "dark", language: "de", accent: "teal" } });
    renderPopup();
    await screen.findByRole("button", { name: "Lesen" });
    expect(screen.getByRole("button", { name: "Löschen" })).toBeTruthy();
    expect(document.documentElement.dataset["theme"]).toBe("dark");
    expect(document.documentElement.dataset["accent"]).toBe("teal");
    expect(document.documentElement.lang).toBe("de");
  });

  describe.each(["light", "dark"] as const)("axe, %s theme", (theme) => {
    it("finds nothing serious with every kind of card", async () => {
      fake({
        cards: [
          card(1, { account: { name: "Work", color: 1 } }),
          card(2, { writesEnabled: false, account: { name: "Home", color: 4 } }),
          card(3, { content: null, account: { name: "Home", color: 4 } }),
        ],
        more: 3,
        appearance: { ...APPEARANCE, theme },
      });
      renderPopup();
      await waitFor(() => {
        expect(cards()).toHaveLength(3);
      });
      await waitFor(() => {
        expect(document.documentElement.dataset["theme"]).toBe(theme);
      });
      expect(await seriousFindings()).toEqual([]);
    });

    it("finds nothing serious with a deleted card", async () => {
      fake({ cards: [card(1, { deleted: true }), card(2)], appearance: { ...APPEARANCE, theme } });
      renderPopup();
      await screen.findByTestId("popup-deleted");
      expect(await seriousFindings()).toEqual([]);
    });
  });
});
