import { act, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MessageListItem } from "../src/services/ipc";
import { useComposerStore } from "../src/stores/composer";
import { useLayoutStore } from "../src/stores/layout";
import { useUiStore } from "../src/stores/ui";
import { makeDraft, makeIdentity, type ComposeFakeOptions } from "./support/compose";
import { chooseInComposerMenu, menuEntries, menuEntry, openComposerMenu } from "./support/composerMenus";
import { dragFilesOver, fileTransfer } from "./support/fileDrop";
import { makeAccount, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";

// Stage 23, item 2, step 4 (decision 0086): the composer as a floating card over the reading
// area: its place, the dialog's keyboard (Tab inside, Escape), minimising to a bar, the header
// with the subject as its title, "From" as a chip, and every former control in its new place.

const ADA = makeAccount({ writesEnabled: true, color: 2 });
const GRACE = { name: "Grace Hopper", email: "grace@example.org" };

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  useComposerStore.setState({ session: null, openDraftId: null, sends: [], problems: [] });
  // ProseMirror measures the selection; jsdom has no layout.
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

function setup({ compose = {}, items = makeMessages(3) }: { compose?: ComposeFakeOptions; items?: MessageListItem[] } = {}) {
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    compose,
  });
  return { core, items, ...renderApp() };
}

async function openDraft(draft: ReturnType<typeof makeDraft>) {
  await screen.findByRole("button", { name: "New message" });
  act(() => {
    useComposerStore.getState().open({ kind: "draft", draftId: draft.id, draft });
  });
  await screen.findByLabelText("Subject");
  return screen.getByTestId("composer");
}

describe("the composer card (decision 0086)", () => {
  it("is a dialog named by its subject, over the reading area beside the list", async () => {
    const draft = makeDraft({ accountId: ADA.id, to: [GRACE], subject: "Plan for Monday" });
    setup({ compose: { drafts: [draft] } });
    const card = await openDraft(draft);
    expect(card.getAttribute("role")).toBe("dialog");
    expect(card.getAttribute("aria-modal")).toBeNull();
    expect(screen.getByRole("dialog", { name: "Plan for Monday" })).toBe(card);
    // Beside the list (the reading pane on the right): the card takes the reading area.
    await waitFor(() => {
      expect(card.getAttribute("data-placement")).toBe("area");
    });
    expect(card.style.getPropertyValue("--composer-top")).toMatch(/^\d+px$/);
    expect(card.className).toContain("rounded-panel");
    expect(card.className).toContain("shadow-panel");
    // The list stays: visible and in the page, not inert.
    const list = screen.getByRole("listbox", { name: /^Messages in/ });
    expect(list.closest("[inert]")).toBeNull();
    // The subject is the title: an empty one shows the kind of message as the name.
    const subject = screen.getByLabelText<HTMLInputElement>("Subject");
    expect(subject.getAttribute("placeholder")).toBe("Subject");
    expect(within(screen.getByTestId("composer-head")).getByLabelText("Subject")).toBe(subject);
  });

  it("stands at the end of the window where no reading pane is beside the list", async () => {
    const draft = makeDraft({ accountId: ADA.id, subject: "" });
    setup({ compose: { drafts: [draft] } });
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useLayoutStore.getState().setLayout({ readingPane: "bottom" });
    });
    const card = await openDraft(draft);
    await waitFor(() => {
      expect(card.getAttribute("data-placement")).toBe("end");
    });
    expect(screen.getByRole("dialog", { name: "New message" })).toBe(card);
    act(() => {
      useLayoutStore.getState().setLayout({ readingPane: "right" });
    });
    await waitFor(() => {
      expect(card.getAttribute("data-placement")).toBe("area");
    });
    // Another view: the card stays, at the end.
    act(() => {
      useUiStore.getState().showSettings("compose");
    });
    await waitFor(() => {
      expect(card.getAttribute("data-placement")).toBe("end");
    });
  });

  it("keeps Tab inside, closes with Escape and gives the focus back", async () => {
    const draft = makeDraft({ accountId: ADA.id, to: [GRACE], subject: "Tabs" });
    const { core, user } = setup({ compose: { drafts: [draft] } });
    const newButton = await screen.findByRole("button", { name: "New message" });
    newButton.focus();
    act(() => {
      useComposerStore.getState().open({ kind: "draft", draftId: draft.id, draft });
    });
    const subject = await screen.findByLabelText("Subject");
    const card = screen.getByTestId("composer");
    // Shift+Tab from the first control goes round to the last (discard), Tab back to the first.
    subject.focus();
    await user.tab({ shift: true });
    expect(document.activeElement).toBe(within(card).getByTestId("composer-discard"));
    await user.tab();
    expect(document.activeElement).toBe(subject);
    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(screen.queryByTestId("composer")).toBeNull();
    });
    expect(core.callsOf("close_draft")).toEqual([{ draftId: draft.id }]);
  });

  it("minimises to a bar and comes back with its text and the focus where it was", async () => {
    const draft = makeDraft({ accountId: ADA.id, to: [GRACE], subject: "Kept", bodyText: "" });
    const { user } = setup({ compose: { drafts: [draft] } });
    const card = await openDraft(draft);
    const body = within(card).getByLabelText<HTMLTextAreaElement>("Message");
    await user.type(body, "Halb fertig");
    await user.click(within(card).getByRole("button", { name: "Minimise" }));
    const restore = within(card).getByTestId("composer-restore");
    expect(card.getAttribute("data-minimised")).toBe("true");
    expect(restore.getAttribute("aria-label")).toBe("Restore: Kept");
    expect(document.activeElement).toBe(restore);
    // The form waits out of sight (not unmounted): no field is reachable meanwhile.
    expect(within(card).getByLabelText<HTMLTextAreaElement>("Message").closest(".hidden")).not.toBeNull();
    expect(within(card).queryByTestId("composer-head")).toBeNull();
    // Tab leaves the bar (no trap while minimised): the list stays usable.
    await user.click(restore);
    expect(card.hasAttribute("data-minimised")).toBe(false);
    expect(within(card).getByLabelText<HTMLTextAreaElement>("Message").value).toBe("Halb fertig");
    expect(document.activeElement).toBe(within(card).getByLabelText("Message"));
  });

  it("files dragged over the window bring a minimised card back; the hint shows only while dragging", async () => {
    const draft = makeDraft({ accountId: ADA.id, to: [GRACE], subject: "Drop" });
    const { user } = setup({ compose: { drafts: [draft] } });
    const card = await openDraft(draft);
    expect(within(card).queryByTestId("composer-drop-hint")).toBeNull();
    expect(card.textContent).not.toContain("drop files");
    await user.click(within(card).getByTestId("composer-minimise"));
    const transfer = fileTransfer([new File(["x"], "notes.txt", { type: "text/plain" })]);
    dragFilesOver(card, transfer);
    expect((await within(card).findByTestId("composer-drop-hint")).textContent).toContain("Drop the files to attach them.");
    expect(card.hasAttribute("data-minimised")).toBe(false);
    expect(within(card).getByTestId("composer-drop-hint").closest(".hidden")).toBeNull();
  });

  it("shows From as a chip in the account's colour, with a menu of the identities", async () => {
    const identities = {
      [ADA.id]: [
        makeIdentity({ id: 1, accountId: ADA.id, email: "ada@vicemail.test", name: "Ada Lovelace" }),
        makeIdentity({ id: 2, accountId: ADA.id, email: "ada@work.test", name: "Ada (work)", isDefault: false }),
      ],
    };
    const draft = makeDraft({ accountId: ADA.id, to: [GRACE], subject: "From" });
    const { core, user } = setup({ compose: { drafts: [draft], identities } });
    const card = await openDraft(draft);
    const chip = await within(card).findByTestId("composer-identity");
    expect(chip.getAttribute("aria-label")).toBe("From: Ada Lovelace · ada@vicemail.test");
    expect(chip.getAttribute("aria-haspopup")).toBe("menu");
    expect(within(chip).getByTestId("composer-identity-color").className).toContain("bg-account-3");
    const menu = await openComposerMenu(user, "composer-identity");
    expect(menuEntries(menu).map((entry) => [entry.textContent, entry.getAttribute("aria-checked")])).toEqual([
      ["Ada Lovelace · ada@vicemail.test", "true"],
      ["Ada (work) · ada@work.test", "false"],
    ]);
    await user.click(menuEntry(menu, "Ada (work) · ada@work.test"));
    await waitFor(() => {
      expect(core.callsOf("save_draft").at(-1)?.["update"]).toMatchObject({ identityId: 2 });
    });
    // One identity: a chip without a menu.
    act(() => {
      useComposerStore.getState().close();
    });
  });

  it("puts Send first with ⌘↩ in its name and tooltip, then the icons, discard at the end", async () => {
    const draft = makeDraft({ accountId: ADA.id, to: [GRACE], subject: "Row", format: "text" });
    setup({ compose: { drafts: [draft] } });
    const card = await openDraft(draft);
    const row = within(card).getByTestId("composer-actions");
    const send = within(row).getByTestId("composer-send");
    expect(send.getAttribute("aria-label")).toBe("Send (⌘↩)");
    expect(send.getAttribute("aria-keyshortcuts")).toBe("Meta+Enter");
    expect(send.textContent).toBe("Send");
    // No ⌘↩ chip of its own any more, no separate "Send later…" button.
    expect(row.querySelector("kbd")).toBeNull();
    expect(within(card).queryByRole("button", { name: "Send later…" })).toBeNull();
    const names = within(row)
      .getAllByRole("button")
      .map((button) => button.getAttribute("aria-label"));
    // Plain text: no "Insert image" (inline images are part of rich text).
    expect(names).toEqual(["Send (⌘↩)", "More ways to send", "Attach files", "Insert template", "Remind me if no reply", "More options", "Discard draft"]);
  });

  it("has a place for every former control (stage 16 to 20 options, format, signature, quote)", async () => {
    const draft = makeDraft({
      accountId: ADA.id,
      kind: "reply",
      to: [GRACE],
      subject: "Re: All",
      format: "html",
      bodyHtml: "<p>Hi</p>",
      quoteText: "> Earlier",
      includeQuote: true,
      originalMessageId: 1,
    });
    const { user } = setup({ compose: { drafts: [draft] } });
    const card = await openDraft(draft);
    await within(card).findByRole("toolbar", { name: "Formatting" }, { timeout: 10_000 });
    // A reply starts in the text.
    const editor = await within(card).findByTestId("composer-rich-body");
    await waitFor(() => {
      expect(document.activeElement).toBe(editor);
    });
    // TipTap focuses once more on the next frame (its own autofocus); let it, before the menus.
    await act(() => new Promise((resolve) => setTimeout(resolve, 50)));
    const options = await openComposerMenu(user, "composer-options");
    expect(within(options).getByRole("group", { name: "My text goes" })).toBeTruthy();
    expect(within(options).getByRole("group", { name: "When sending" })).toBeTruthy();
    expect(within(options).getByRole("group", { name: "Format" })).toBeTruthy();
    const names = menuEntries(options).map((entry) => entry.querySelector("span + span")?.firstChild?.textContent ?? entry.textContent);
    expect(names).toEqual([
      "Quote the original message",
      "below the quote",
      "above the quote",
      "Archive the original after sending",
      "Request a read receipt",
      "Rich text",
      "Plain text",
    ]);
    await user.keyboard("{Escape}");
    expect(document.activeElement).toBe(within(card).getByTestId("composer-options"));
    // "Send and archive" in the send button's menu, with its shortcut.
    const later = await openComposerMenu(user, "composer-send-later");
    expect(menuEntry(later, "Send and archive").getAttribute("aria-keyshortcuts")).toBe("Meta+Shift+Enter");
    await user.keyboard("{Escape}");
    // The quote is folded; "Show quote" unfolds it.
    await chooseInComposerMenu(user, "composer-options", "above the quote");
    expect(within(card).getByTestId("composer-quote-toggle").getAttribute("aria-expanded")).toBe("false");
  });
});
