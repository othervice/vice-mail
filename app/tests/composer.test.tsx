import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { mockWindows } from "@tauri-apps/api/mocks";
import userEvent from "@testing-library/user-event";
import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "../src/app/App";
import { createQueryClient } from "../src/app/queryClient";
import { DRAFT_SAVE_DELAY_MS } from "../src/features/composer/Composer";
import { createI18n } from "../src/i18n";
import type { DraftUpdate, MessageListItem } from "../src/services/ipc";
import { useComposerStore } from "../src/stores/composer";
import { useUiStore } from "../src/stores/ui";
import { makeDraft, makeIdentity, makeOutgoing, type ComposeFakeOptions } from "./support/compose";
import { ipcError, makeAccount, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";
import { optionName } from "./support/names";

const ADA = makeAccount({ writesEnabled: true });
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

/** The list virtualizer needs a viewport height (jsdom has no layout). */
beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

interface SetupOptions {
  account?: typeof ADA;
  compose?: ComposeFakeOptions;
  items?: MessageListItem[];
}

function setup({ account = ADA, compose = {}, items = makeMessages(3) }: SetupOptions = {}) {
  const core = mockCore({
    accounts: [account],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    compose,
  });
  return { core, items, ...renderApp() };
}

const saves = (core: ReturnType<typeof mockCore>) => core.callsOf("save_draft").map((c) => c["update"] as DraftUpdate);

async function openNew(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "New message" }));
  await screen.findByLabelText("Subject");
  return screen.getByTestId("composer");
}

const toField = () => screen.getByRole("combobox", { name: "To" });
const chips = (testId = "composer-to") => within(screen.getByTestId(testId)).queryAllByTestId("recipient-chip");

describe("composer: writing and drafts", () => {
  it("opens from the header, turns typed addresses into chips and saves on leaving a field", async () => {
    const { core, user } = setup();
    await openNew(user);
    expect(core.callsOf("create_draft")[0]?.["request"]).toEqual({ kind: "new", accountId: ADA.id, identityId: null, to: [] });

    await user.type(toField(), "Grace Hopper <grace@example.org>, lin@example.net;");
    expect(chips().map((c) => c.textContent)).toEqual(
      expect.arrayContaining([expect.stringContaining("Grace Hopper"), expect.stringContaining("lin@example.net")]),
    );
    await user.type(screen.getByLabelText("Subject"), "Größe 🧾");
    await user.tab();
    await waitFor(() => {
      expect(saves(core).at(-1)?.subject).toBe("Größe 🧾");
    });
    expect(saves(core).at(-1)?.to).toEqual([
      { name: "Grace Hopper", email: "grace@example.org" },
      { name: null, email: "lin@example.net" },
    ]);
    expect(screen.getByTestId("composer-save-state")?.textContent).toContain("Saved");
  });

  it("shows Cc and Bcc on request and saves blind copies separately", async () => {
    const { core, user } = setup();
    await openNew(user);
    expect(screen.queryByRole("combobox", { name: "Bcc" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Add Bcc recipients" }));
    const bcc = screen.getByRole("combobox", { name: "Bcc" });
    expect(document.activeElement).toBe(bcc);
    await user.type(bcc, "geheim@example.org,");
    await user.tab();
    await waitFor(() => {
      expect(saves(core).at(-1)?.bcc).toEqual([{ name: null, email: "geheim@example.org" }]);
    });
    expect(saves(core).at(-1)?.to).toEqual([]);
    expect(screen.queryByRole("button", { name: "Add Bcc recipients" })).toBeNull();
    expect(screen.getByRole("button", { name: "Add Cc recipients" })).toBeTruthy();
  });

  it("marks what cannot be an address and removes the last chip with Backspace", async () => {
    const { user } = setup();
    await openNew(user);
    await user.type(toField(), "not-an-address{Enter}");
    const [chip] = chips();
    expect(chip?.getAttribute("data-invalid")).toBe("true");
    expect(toField()?.getAttribute("aria-invalid")).toBe("true");
    await user.type(toField(), "{Backspace}");
    expect(chips()).toHaveLength(0);
  });

  it("saves while typing only after the pause, not on every key", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      mockCore({ accounts: [ADA], mailboxes: standardMailboxes(), messages: { 1: [] }, compose: {} });
      const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
      render(<App queryClient={createQueryClient()} i18n={createI18n("en")} />);
      await user.click(await screen.findByRole("button", { name: "New message" }));
      const body = await screen.findByLabelText("Message");
      fireEvent.change(body, { target: { value: "Hallo" } });
      fireEvent.change(body, { target: { value: "Hallo Grace" } });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(DRAFT_SAVE_DELAY_MS - 500);
      });
      expect(useComposerStore.getState().session).not.toBeNull();
      // Nothing saved yet: the pause has not passed.
      expect(screen.getByTestId("composer-save-state")?.getAttribute("data-state")).toBe("idle");
      await act(async () => {
        await vi.advanceTimersByTimeAsync(600);
      });
      await waitFor(() => {
        expect(screen.getByTestId("composer-save-state")?.getAttribute("data-state")).toBe("saved");
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("closing saves the draft and hands it to the server copy", async () => {
    const { core, user } = setup();
    await openNew(user);
    await user.type(screen.getByLabelText("Message"), "Bis morgen");
    await user.click(screen.getByRole("button", { name: "Close (the draft is kept)" }));
    await waitFor(() => {
      expect(screen.queryByTestId("composer")).toBeNull();
    });
    expect(saves(core).at(-1)?.bodyText).toBe("Bis morgen");
    expect(core.callsOf("close_draft")).toHaveLength(1);
    expect(core.callsOf("delete_draft")).toHaveLength(0);
  });

  it("opening another message saves the first draft and hands it to the server copy", async () => {
    const { core, user } = setup();
    await openNew(user);
    await user.type(screen.getByLabelText("Subject"), "Erster");
    const first = core.callsOf("create_draft").length;
    act(() => {
      useComposerStore.getState().open({ kind: "new", accountId: ADA.id });
    });
    await waitFor(() => {
      expect(core.callsOf("close_draft")).toHaveLength(1);
    });
    expect(saves(core).at(-1)?.subject).toBe("Erster");
    await waitFor(() => {
      expect(core.callsOf("create_draft")).toHaveLength(first + 1);
    });
  });

  it("creates one draft per opening, also under StrictMode", async () => {
    const core = mockCore({ accounts: [ADA], mailboxes: standardMailboxes(), messages: { 1: [] }, compose: {} });
    const user = userEvent.setup();
    render(
      <StrictMode>
        <App queryClient={createQueryClient()} i18n={createI18n("en")} />
      </StrictMode>,
    );
    await user.click(await screen.findByRole("button", { name: "New message" }));
    await screen.findByLabelText("Subject");
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(core.callsOf("create_draft")).toHaveLength(1);
    expect(core.callsOf("close_draft")).toHaveLength(0);
  });

  it("gives the focus back to where it was when it closes", async () => {
    const { user } = setup();
    const button = await screen.findByRole("button", { name: "New message" });
    await user.click(button);
    await screen.findByLabelText("Subject");
    expect(document.activeElement).toBe(toField());
    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(screen.queryByTestId("composer")).toBeNull();
    });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "New message" }));
  });

  it("discards only after the confirmation", async () => {
    const { core, user } = setup();
    await openNew(user);
    await user.click(screen.getByRole("button", { name: "Discard draft" }));
    const dialog = await screen.findByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Keep editing" }));
    expect(core.callsOf("delete_draft")).toHaveLength(0);
    expect(screen.getByTestId("composer")).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "Discard draft" }));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Discard" }));
    await waitFor(() => {
      expect(screen.queryByTestId("composer")).toBeNull();
    });
    expect(core.callsOf("delete_draft")).toHaveLength(1);
    expect(core.callsOf("close_draft")).toHaveLength(0);
  });

  it("suggests addresses of earlier mail and takes one with the keyboard", async () => {
    const { core, user } = setup({
      compose: { contacts: [{ name: "Grace Hopper", email: "grace@example.org", sent: 4 }, { name: null, email: "gremlin@example.net", sent: 1 }] },
    });
    await openNew(user);
    await user.type(toField(), "gra");
    const option = await screen.findByRole("option", { name: /Grace Hopper/ });
    expect(toField()?.getAttribute("aria-expanded")).toBe("true");
    expect(option?.getAttribute("aria-selected")).toBe("true");
    await user.keyboard("{Enter}");
    expect(chips().map((c) => c.textContent)).toEqual([expect.stringContaining("Grace Hopper")]);
    expect(core.callsOf("search_contacts").at(-1)).toEqual({ query: "gra", limit: 8 });
  });

  it("shows the identity's signature and switches it off on request", async () => {
    const identities = {
      [ADA.id]: [
        makeIdentity({ id: 1, email: ADA.email, name: "Ada", signature: "Ada Lovelace\nAnalytical Engines" }),
        makeIdentity({ id: 2, email: "ada@work.test", name: "Ada (work)", isDefault: false, signature: null }),
      ],
    };
    const { core, user } = setup({ compose: { identities } });
    await openNew(user);
    const signature = await screen.findByTestId("composer-signature");
    expect(signature.textContent).toBe("-- \nAda Lovelace\nAnalytical Engines");
    await user.selectOptions(screen.getByLabelText("From"), "2");
    expect(screen.queryByTestId("composer-signature")).toBeNull();
    await user.tab();
    await waitFor(() => {
      expect(saves(core).at(-1)?.identityId).toBe(2);
    });
  });

  it("attaches files through the core's dialog, warns about size and removes them", async () => {
    const big = { id: "a1", fileName: "Prüfbericht 🧾.pdf", mime: "application/pdf", size: 16 * 1024 * 1024, contentId: null };
    const { core, user } = setup({ compose: { picked: [big] } });
    await openNew(user);
    await user.click(screen.getByRole("button", { name: "Attach files" }));
    const attachment = await screen.findByTestId("composer-attachment");
    expect(attachment?.textContent).toContain("Prüfbericht 🧾.pdf");
    expect(screen.getByTestId("composer-attachments-large")?.textContent).toContain("The attachments are large");
    expect(core.callsOf("pick_attachments")).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: "Remove Prüfbericht 🧾.pdf" }));
    await waitFor(() => {
      expect(screen.queryByTestId("composer-attachment")).toBeNull();
    });
    expect(core.callsOf("remove_draft_attachment")[0]).toMatchObject({ attachmentId: "a1" });
  });

  it("files dropped on the window become attachments, with the shell's paths", async () => {
    const { core, user } = setup();
    mockWindows("main");
    await openNew(user);
    const paths = ["/Users/ada/Prüfbericht 🧾.pdf"];
    // The listener of the shell's drag-and-drop event is registered asynchronously.
    await waitFor(async () => {
      await act(async () => {
        await emit("tauri://drag-enter", { paths, position: { x: 10, y: 10 } });
      });
      expect(screen.getByTestId("composer-drop-hint")).toBeTruthy();
    });
    await act(async () => {
      await emit("tauri://drag-drop", { paths, position: { x: 10, y: 10 } });
    });
    await waitFor(() => {
      expect(core.callsOf("add_dropped_attachments")[0]).toMatchObject({ paths });
    });
    expect((await screen.findByTestId("composer-attachment")).textContent).toContain("Prüfbericht 🧾.pdf");
    expect(screen.queryByTestId("composer-drop-hint")).toBeNull();
  });

  it("offers the drafts left from the last session once at the start", async () => {
    const left = makeDraft({ accountId: ADA.id, subject: "Angebot", bodyText: "Entwurf von gestern", to: [{ name: null, email: "kunde@example.org" }] });
    const { user } = setup({ compose: { drafts: [left] } });
    const offer = await screen.findByTestId("draft-restore");
    expect(offer?.textContent).toContain("1 unsent draft is waiting.");
    await user.click(within(offer).getByRole("button", { name: "Open the newest" }));
    expect((await screen.findByLabelText<HTMLInputElement>("Subject")).value).toBe("Angebot");
    expect((screen.getByLabelText<HTMLInputElement>("Message")).value).toBe("Entwurf von gestern");
    expect(screen.queryByTestId("draft-restore")).toBeNull();
  });

  it("a frozen draft cannot be edited until sending is cancelled", async () => {
    const frozen = makeDraft({ accountId: ADA.id, subject: "Wartet", state: "sending", to: [{ name: null, email: "a@example.org" }] });
    const { core, user } = setup({ compose: { drafts: [frozen], outgoing: [makeOutgoing({ operationId: 7, draftId: frozen.id, state: "failed" })] } });
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useComposerStore.getState().open({ kind: "draft", draftId: frozen.id });
    });
    await screen.findByTestId("composer-frozen");
    expect((screen.getByLabelText<HTMLInputElement>("Subject")).disabled).toBe(true);
    expect(screen.getByTestId<HTMLButtonElement>("composer-send").disabled).toBe(true);
    await user.click(screen.getByRole("button", { name: "Cancel sending and edit" }));
    await waitFor(() => {
      expect((screen.getByLabelText<HTMLInputElement>("Subject")).disabled).toBe(false);
    });
    expect(core.callsOf("cancel_send")[0]).toEqual({ draftId: frozen.id });
  });

  it("a draft of another program whose original stays in Drafts says so; others do not", async () => {
    const kept = makeDraft({ accountId: ADA.id, subject: "Logo", originalKept: true, format: "html", bodyHtml: "<p>Hallo</p>" });
    const plain = makeDraft({ accountId: ADA.id, subject: "Ohne Logo", format: "html", bodyHtml: "<p>Hallo</p>" });
    setup({ compose: { drafts: [kept, plain], fromMessage: { 41: kept } } });
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useComposerStore.getState().open({ kind: "message", messageId: 41 });
    });
    const notice = await screen.findByTestId("composer-original-kept");
    expect(notice.textContent).toContain("The original draft stays unchanged");
    expect(notice.textContent).toContain("your version is saved next to it as a separate draft");
    act(() => {
      useComposerStore.getState().open({ kind: "draft", draftId: plain.id });
    });
    await waitFor(() => {
      expect((screen.getByLabelText<HTMLInputElement>("Subject")).value).toBe("Ohne Logo");
    });
    expect(screen.queryByTestId("composer-original-kept")).toBeNull();
  });

  it("rich text: the editor gets the draft's HTML and switching to plain text keeps the words", async () => {
    const rich = makeDraft({ accountId: ADA.id, format: "html", bodyHtml: "<p>Hallo <strong>Grace</strong></p><ul><li>eins</li></ul>" });
    const { user } = setup({ compose: { drafts: [rich] } });
    await user.click(await screen.findByRole("button", { name: "Open the newest" }));
    const editor = await screen.findByTestId("composer-rich-body");
    expect(editor.getAttribute("aria-label")).toBe("Message");
    expect(editor.querySelector("strong")?.textContent).toBe("Grace");
    expect(screen.getByRole("toolbar", { name: "Formatting" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Bold" })?.hasAttribute("aria-pressed")).toBe(true);
    await user.click(screen.getByRole("button", { name: "Plain text" }));
    expect((screen.getByLabelText<HTMLInputElement>("Message")).value).toBe("Hallo Grace\n- eins");
  });
});

describe("composer: rich text typing", () => {
  // ProseMirror measures the selection; jsdom has no layout.
  beforeEach(() => {
    const rects = () => Object.assign([], { item: () => null }) as unknown as DOMRectList;
    document.elementFromPoint = () => null;
    Range.prototype.getClientRects = rects;
    Range.prototype.getBoundingClientRect = () => new DOMRect();
    (Text.prototype as unknown as { getClientRects: () => DOMRectList }).getClientRects = rects;
  });
  afterEach(() => {
    const doc = document as unknown as Record<string, unknown>;
    delete doc["elementFromPoint"];
    delete (Range.prototype as unknown as Record<string, unknown>)["getClientRects"];
    delete (Range.prototype as unknown as Record<string, unknown>)["getBoundingClientRect"];
    delete (Text.prototype as unknown as Record<string, unknown>)["getClientRects"];
  });

  it("what the user types in the rich text body is saved as editor HTML", async () => {
    const { core, user } = setup();
    await openNew(user);
    await user.click(screen.getByRole("button", { name: "Rich text" }));
    const editor = await screen.findByTestId("composer-rich-body");
    await user.click(editor);
    await user.type(editor, "Hallo");
    await user.click(screen.getByRole("button", { name: "Bold" }));
    expect(screen.getByRole("button", { name: "Bold" }).getAttribute("aria-pressed")).toBe("true");
    await user.type(editor, " Welt");
    await user.click(screen.getByLabelText("Subject"));
    await waitFor(() => {
      const last = saves(core).at(-1);
      expect(last?.format).toBe("html");
      expect(last?.bodyHtml).toContain("Hallo");
    });
    expect(saves(core).at(-1)?.bodyHtml).toMatch(/<strong>\s*Welt<\/strong>/);
  });
});

describe("composer: replying and forwarding", () => {
  it("Reply in the reading pane opens a prepared reply with the quote", async () => {
    const items = makeMessages(2);
    const first = items[0];
    if (!first) throw new Error("no message");
    const { core, user } = setup({
      items,
      compose: {
        replies: {
          [first.id]: {
            accountId: ADA.id,
            subject: `Re: ${first.subject}`,
            to: first.from ? [first.from] : [],
            quoteText: "On Monday, Grace Hopper wrote:\n> Hello",
            quoteHtml: "<blockquote>Hello</blockquote>",
          },
        },
      },
    });
    const listbox = await screen.findByRole("listbox", { name: /^Messages in/ });
    const row = within(listbox)
      .getAllByRole("option")
      .find((o) => optionName(o).includes(first.subject));
    if (!row) throw new Error("no row");
    await user.click(row);
    await user.click(await screen.findByRole("button", { name: "Reply" }));
    expect((await screen.findByLabelText<HTMLInputElement>("Subject")).value).toBe(`Re: ${first.subject}`);
    expect(core.callsOf("create_draft")[0]?.["request"]).toEqual({ kind: "reply", messageId: first.id, language: "en" });
    expect(screen.getByTestId("composer-quote")?.textContent).toContain("Grace Hopper wrote:");
    expect(chips().map((c) => c.textContent)).toEqual([expect.stringContaining(first.from?.name ?? "")]);
    await user.click(screen.getByRole("checkbox", { name: "Quote the original message" }));
    expect(screen.queryByTestId("composer-quote")).toBeNull();
  });

  it("places the answer where quotePosition says (core: the position of the answer)", async () => {
    const items = makeMessages(1);
    const first = items[0];
    if (!first) throw new Error("no message");
    const { user } = setup({
      items,
      compose: {
        replies: {
          [first.id]: { accountId: ADA.id, subject: `Re: ${first.subject}`, to: [], quoteText: "> Hello", quoteHtml: null },
        },
      },
    });
    const listbox = await screen.findByRole("listbox", { name: /^Messages in/ });
    const row = within(listbox).getAllByRole("option")[0];
    if (!row) throw new Error("no row");
    await user.click(row);
    await user.click(await screen.findByRole("button", { name: "Reply" }));
    await screen.findByTestId("composer-quote");
    const subject = screen.getByLabelText("Subject");
    const quoteFollows = () =>
      Boolean(subject.compareDocumentPosition(screen.getByTestId("composer-quote")) & Node.DOCUMENT_POSITION_FOLLOWING);
    const position = screen.getByTestId("composer-quote-position");
    // "above": the answer above the quote, so the quote comes last.
    await user.selectOptions(position, "above");
    const quote = screen.getByTestId("composer-quote");
    const body = screen.getByTestId("composer").querySelector("textarea, [contenteditable]");
    if (!body) throw new Error("no body");
    expect(quoteFollows()).toBe(true);
    expect(body.compareDocumentPosition(quote) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.getByRole("option", { name: "above the quote" })).toBeTruthy();
    // "below": the quote first, then the answer.
    await user.selectOptions(position, "below");
    const moved = screen.getByTestId("composer-quote");
    expect(body.compareDocumentPosition(moved) & Node.DOCUMENT_POSITION_PRECEDING).toBeTruthy();
  });

  it("an error while preparing the reply is shown with a way to try again", async () => {
    const items = makeMessages(1);
    const { user } = setup({ items, compose: { replies: {} } });
    const first = items[0];
    if (!first) throw new Error("no message");
    act(() => {
      useComposerStore.getState().open({ kind: "forward", messageId: first.id });
    });
    const error = await screen.findByTestId("composer-open-error");
    expect(error?.getAttribute("data-error-code")).toBe("not_found");
    expect(within(error).getByRole("button", { name: "Try again" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Close (the draft is kept)" }));
    expect(screen.queryByTestId("composer")).toBeNull();
  });
});

describe("composer: sending", () => {
  async function writeAndSend(user: ReturnType<typeof userEvent.setup>, to = "grace@example.org") {
    await openNew(user);
    if (to) await user.type(toField(), `${to},`);
    await user.type(screen.getByLabelText("Subject"), "Plan");
    await user.click(screen.getByTestId("composer-send"));
  }

  it("saves, sends with a fresh request id, closes, and Undo brings the draft back", async () => {
    const { core, user } = setup();
    await writeAndSend(user);
    await waitFor(() => {
      expect(screen.queryByTestId("composer")).toBeNull();
    });
    const [send] = core.callsOf("send_draft");
    expect(send?.["requestId"]).toMatch(UUID);
    expect(saves(core).at(-1)?.to).toEqual([{ name: null, email: "grace@example.org" }]);
    const notice = await screen.findByTestId("send-notice");
    expect(notice?.getAttribute("data-send-state")).toBe("scheduled");
    expect(notice?.textContent).toMatch(/Sending in (10|9) seconds…/);

    await user.click(within(notice).getByRole("button", { name: "Undo" }));
    expect((await screen.findByLabelText<HTMLInputElement>("Subject")).value).toBe("Plan");
    expect(core.callsOf("cancel_send")[0]).toEqual({ draftId: send?.["draftId"] });
    expect(screen.queryByTestId("send-notice")).toBeNull();
    expect(chips().map((c) => c.textContent)).toEqual([expect.stringContaining("grace@example.org")]);
  });

  it("Cmd+Enter sends from the message body", async () => {
    const { core, user } = setup();
    await openNew(user);
    await user.type(toField(), "grace@example.org,");
    await user.type(screen.getByLabelText("Message"), "Kurz{Meta>}{Enter}{/Meta}");
    await waitFor(() => {
      expect(core.callsOf("send_draft")).toHaveLength(1);
    });
  });

  it("a send without a valid recipient shows the core's error and marks the address", async () => {
    const { core, user } = setup();
    await writeAndSend(user, "grace@@example");
    const error = await screen.findByTestId("composer-send-error");
    expect(error?.getAttribute("data-error-code")).toBe("invalid_address");
    expect(error?.textContent).toContain("An email address is not valid.");
    expect(error?.textContent).toContain("Check the recipients.");
    expect(chips()[0]?.getAttribute("data-invalid")).toBe("true");
    expect(screen.getByTestId("composer")).toBeTruthy();
    expect(core.compose.outgoing).toHaveLength(0);
  });

  it("no recipient at all: invalid_address, nothing queued", async () => {
    const { user } = setup();
    await writeAndSend(user, "");
    expect((await screen.findByTestId("composer-send-error"))?.getAttribute("data-error-code")).toBe("invalid_address");
  });

  it("a read-only account keeps the draft and explains where to allow changes", async () => {
    const { user } = setup({ account: makeAccount({ writesEnabled: false }) });
    await writeAndSend(user);
    const error = await screen.findByTestId("composer-send-error");
    expect(error?.textContent).toContain("Changes are turned off for this account.");
    await user.click(within(error).getByRole("button", { name: "Allow changes…" }));
    expect(useUiStore.getState().view).toEqual({ kind: "settings", section: "accounts" });
  });

  it("follows the states from the core: sending, then a failure with Retry, Edit and the outbox", async () => {
    const { core, user } = setup();
    await writeAndSend(user);
    const notice = await screen.findByTestId("send-notice");
    const operationId = core.compose.outgoing[0]?.operationId ?? 0;
    await emitCore({ type: "outgoingChanged", accountId: ADA.id, mails: [core.compose.setOutgoing(operationId, { state: "sending" })] });
    expect(notice?.textContent).toContain("Sending…");
    await emitCore({
      type: "outgoingChanged",
      accountId: ADA.id,
      mails: [core.compose.setOutgoing(operationId, { state: "failed", error: ipcError("send_rejected", "550 no such user") })],
    });
    const alert = screen.getByTestId("send-notice");
    await waitFor(() => {
      expect(alert.getAttribute("data-send-state")).toBe("failed");
    });
    expect(alert.getAttribute("role")).toBe("alert");
    expect(alert?.textContent).toContain("The mail server refused the message.");
    expect(within(alert).getByRole("button", { name: "Retry" })).toBeTruthy();
    expect(within(alert).getByRole("button", { name: "Show outbox" })).toBeTruthy();
    // Retry is covered by the outbox view; here "Edit" takes the failed send back into the composer.
    await user.click(within(alert).getByRole("button", { name: "Edit" }));
    expect((await screen.findByLabelText<HTMLInputElement>("Subject")).value).toBe("Plan");
  });

  it("a sent message says so, also when there was no Sent folder for a copy", async () => {
    const { core, user } = setup();
    await writeAndSend(user);
    const notice = await screen.findByTestId("send-notice");
    const operationId = core.compose.outgoing[0]?.operationId ?? 0;
    await emitCore({
      type: "outgoingChanged",
      accountId: ADA.id,
      mails: [core.compose.setOutgoing(operationId, { state: "sent", notice: "no_sent_copy" })],
    });
    expect(notice?.textContent).toContain("Message sent");
    expect(notice?.textContent).toContain("No Sent folder was found");
  });

  it("an unclear outcome is never repeated without the user's confirmed choice", async () => {
    const mail = makeOutgoing({ operationId: 42, state: "unclear", subject: "Rechnung", error: ipcError("send_unclear") });
    const { core, user } = setup({ compose: { outgoing: [mail] } });
    await screen.findByRole("button", { name: "New message" });
    await emitCore({ type: "sendNeedsDecision", accountId: ADA.id, mail });
    const notice = await screen.findByTestId("send-notice");
    expect(notice?.getAttribute("role")).toBe("alert");
    expect(notice?.textContent).toContain("It is unclear whether the message was sent.");
    expect(core.callsOf("resolve_unclear_send")).toHaveLength(0);
    expect(core.callsOf("retry_operation")).toHaveLength(0);

    await user.click(within(notice).getByRole("button", { name: "Send again…" }));
    expect(notice?.textContent).toContain("the recipients get the message twice");
    expect(core.callsOf("resolve_unclear_send")).toHaveLength(0);
    await user.click(within(notice).getByRole("button", { name: "Cancel" }));
    await user.click(within(notice).getByRole("button", { name: "Send again…" }));
    await user.click(within(notice).getByRole("button", { name: "Send again" }));
    await waitFor(() => {
      expect(core.callsOf("resolve_unclear_send")).toEqual([{ operationId: 42, decision: "send_again" }]);
    });
  });

  it("“It arrived” settles an unclear send without sending", async () => {
    const mail = makeOutgoing({ operationId: 43, state: "unclear" });
    const { core, user } = setup({ compose: { outgoing: [mail] } });
    await screen.findByRole("button", { name: "New message" });
    await emitCore({ type: "sendNeedsDecision", accountId: ADA.id, mail });
    await user.click(within(await screen.findByTestId("send-notice")).getByRole("button", { name: "It arrived" }));
    await waitFor(() => {
      expect(core.callsOf("resolve_unclear_send")).toEqual([{ operationId: 43, decision: "treat_as_sent" }]);
    });
    expect((await screen.findByTestId("send-notice"))?.getAttribute("data-send-state")).toBe("sent");
  });

  it("too late to undo: the notice says so", async () => {
    const { core, user } = setup();
    await writeAndSend(user);
    const notice = await screen.findByTestId("send-notice");
    const operationId = core.compose.outgoing[0]?.operationId ?? 0;
    core.compose.setOutgoing(operationId, { state: "sending" });
    await user.click(within(notice).getByRole("button", { name: "Undo" }));
    const problem = await screen.findByTestId("composer-problem");
    expect(problem?.getAttribute("data-error-code")).toBe("send_in_progress");
    expect(problem?.textContent).toContain("Too late to undo");
  });
});

describe("outbox view", () => {
  it("lists every send with its state, error and the user's choices", async () => {
    const outgoing = [
      makeOutgoing({ operationId: 3, draftId: "d3", subject: "Abgelehnt", state: "failed", error: ipcError("send_rejected") }),
      makeOutgoing({ operationId: 2, draftId: "d2", subject: "Unklar", state: "unclear", error: ipcError("send_unclear") }),
      makeOutgoing({ operationId: 1, subject: "Offline", state: "queued", error: ipcError("connection_refused") }),
    ];
    const { core, user } = setup({ compose: { outgoing } });
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useUiStore.getState().showOutbox();
    });
    const list = await screen.findByTestId("outbox-list");
    const rows = within(list).getAllByTestId("outgoing-row");
    expect(rows.map((r) => r.getAttribute("data-send-state"))).toEqual(["failed", "unclear", "queued"]);
    expect(rows[0]?.textContent).toContain("The mail server refused the message.");
    expect(rows[2]?.textContent).toContain("goes out by itself once the account is connected again");

    const [failed, unclear] = rows;
    if (!failed || !unclear) throw new Error("rows missing");
    await user.click(within(failed).getByRole("button", { name: "Retry" }));
    await waitFor(() => {
      expect(core.callsOf("retry_operation")).toEqual([{ operationId: 3 }]);
    });
    await user.click(within(unclear).getByRole("button", { name: "Send again…" }));
    expect(core.callsOf("resolve_unclear_send")).toHaveLength(0);
    await user.click(within(unclear).getByRole("button", { name: "Send again" }));
    await waitFor(() => {
      expect(core.callsOf("resolve_unclear_send")).toEqual([{ operationId: 2, decision: "send_again" }]);
    });
  });

  it("is empty without sends", async () => {
    setup();
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useUiStore.getState().showOutbox();
    });
    expect(await screen.findByText("Nothing in the outbox")).toBeTruthy();
  });
});

describe("writing settings", () => {
  it("changes the undo delay and offers the default signatures per address", async () => {
    const identities = { [ADA.id]: [makeIdentity({ id: 1, email: ADA.email, name: "Ada" })] };
    const { core, user } = setup({ compose: { identities } });
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useUiStore.getState().showSettings("compose");
    });
    const delay = await screen.findByLabelText("Undo send");
    await waitFor(() => {
      expect((delay as HTMLInputElement).disabled).toBe(false);
    });
    await user.selectOptions(delay, "30");
    await waitFor(() => {
      // Stage 8: the default quote position goes along, so saving the delay keeps it.
      expect(core.callsOf("set_compose_settings")).toEqual([{ settings: { sendDelaySeconds: 30, defaultQuotePosition: "above" } }]);
    });
    // Stage 10: the plain text signature per address became a choice from the signature pool
    // (tests/snippets-settings.test.tsx covers it).
    const defaults = await screen.findByTestId("signature-defaults");
    expect(within(defaults).getByRole("combobox", { name: "New messages" })).toBeTruthy();
    expect(within(defaults).getByRole("combobox", { name: "Replies and forwards" })).toBeTruthy();
  });
});
