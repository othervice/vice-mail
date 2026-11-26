import { act, screen, waitFor, within } from "@testing-library/react";
import { mockWindows } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { parseWindowRoute } from "../src/features/windows/route";
import { openComposer, useComposerStore } from "../src/stores/composer";
import { useUiStore } from "../src/stores/ui";
import { ipcError, makeAccount, makeMailbox, makeMessages, makeView, mockCore, standardMailboxes, type Failure } from "./support/core";
import { makeDraft } from "./support/compose";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";
import { toolbarAction } from "./support/reading";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

const ACCOUNT = makeAccount({ writesEnabled: true });
const DRAFT_A = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const DRAFT_B = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

/** The page of a window of its own; the shell gives it its label (`getCurrentWindow`). */
function openWindow(search: string) {
  window.history.replaceState(null, "", `/${search}`);
  const params = new URLSearchParams(search);
  mockWindows(params.get("window") === "composer" ? `composer-${params.get("draftId") ?? ""}` : "message-test");
}

function setup(options: { canCloseWindow?: boolean; failures?: Record<string, Failure>; drafts?: ReturnType<typeof makeDraft>[] } = {}) {
  const items = makeMessages(3);
  const core = mockCore({
    accounts: [ACCOUNT],
    mailboxes: [...standardMailboxes(), makeMailbox({ id: 7, path: "Archive", role: "archive", roleSource: "server" })],
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    ...(options.failures && { failures: options.failures }),
    compose: {
      drafts: options.drafts ?? [
        makeDraft({ id: DRAFT_A, subject: "First draft", to: [{ name: null, email: "grace@example.org" }] }),
        makeDraft({ id: DRAFT_B, subject: "Second draft" }),
      ],
    },
    ui: { canCloseWindow: options.canCloseWindow ?? false },
  });
  return { core, items };
}

describe("window routes (decision 0025)", () => {
  it("reads which window this is from the query and refuses broken ones", () => {
    expect(parseWindowRoute("")).toEqual({ kind: "main" });
    expect(parseWindowRoute(`?window=message&accountId=${ACCOUNT.id}&messageId=42`)).toEqual({
      kind: "message",
      accountId: ACCOUNT.id,
      messageId: 42,
    });
    expect(parseWindowRoute(`?window=composer&draftId=${DRAFT_A.toUpperCase()}`)).toEqual({ kind: "composer", draftId: DRAFT_A });
    expect(parseWindowRoute(`?window=message&accountId=nope&messageId=42`)).toEqual({ kind: "invalid" });
    expect(parseWindowRoute(`?window=message&accountId=${ACCOUNT.id}&messageId=4x`)).toEqual({ kind: "invalid" });
    expect(parseWindowRoute(`?window=message&accountId=${ACCOUNT.id}&messageId=-1`)).toEqual({ kind: "invalid" });
    expect(parseWindowRoute("?window=composer&draftId=draft-1")).toEqual({ kind: "invalid" });
    expect(parseWindowRoute("?window=settings")).toEqual({ kind: "invalid" });
  });

  it("says so when the window was opened with a query it does not understand", async () => {
    setup();
    openWindow("?window=composer&draftId=not-a-uuid");
    renderApp();
    expect(await screen.findByRole("heading", { name: "This window has nothing to show" })).toBeTruthy();
    expect(screen.queryByRole("navigation", { name: "Views" })).toBeNull();
  });
});

describe("the message window", () => {
  it("shows one message with its toolbar and actions, without rail or list", async () => {
    const { core, items } = setup();
    const message = items[1];
    if (!message) throw new Error("no message");
    openWindow(`?window=message&accountId=${ACCOUNT.id}&messageId=${String(message.id)}`);
    const { user } = renderApp();
    const pane = await screen.findByRole("region", { name: "Message" });
    await waitFor(() => {
      expect(pane.textContent).toContain(message.subject);
    });
    expect(screen.queryByRole("navigation", { name: "Views" })).toBeNull();
    expect(screen.queryByRole("listbox", { name: /^Messages in/ })).toBeNull();
    // It asked for its own layout (the shell knows which window asks).
    expect(core.callsOf("get_window_layout")).toHaveLength(1);
    const toolbar = within(pane).getByRole("toolbar", { name: "Message actions" });
    expect(within(toolbar).queryByRole("button", { name: "Open in new window" })).toBeNull();
    await user.click(within(toolbar).getByRole("button", { name: "Archive" }));
    await waitFor(() => {
      expect(core.callsOf("apply_action")).toHaveLength(1);
    });
    expect(core.callsOf("apply_action")[0]).toMatchObject({ action: { type: "archive" }, messages: [{ mailboxId: 1, messageId: message.id }] });
    // The shortcuts work here too.
    await user.keyboard("r");
    expect(await screen.findByTestId("composer")).toBeTruthy();
  });

  it("says the message is gone instead of showing an empty pane", async () => {
    setup();
    openWindow(`?window=message&accountId=${ACCOUNT.id}&messageId=999`);
    renderApp();
    expect(await screen.findByRole("heading", { name: "This message is no longer here" })).toBeTruthy();
  });

  it("asks the shell to close the window from the pane's close button and says how when it may not", async () => {
    const { core, items } = setup();
    openWindow(`?window=message&accountId=${ACCOUNT.id}&messageId=${String(items[0]?.id)}`);
    const { user } = renderApp();
    const pane = await screen.findByRole("region", { name: "Message" });
    await user.click(within(pane).getByRole("button", { name: "Close message" }));
    const notice = await screen.findByTestId("window-close-failed");
    expect(notice.textContent).toContain("Close it with its close button.");
    expect(core.ui.closeRequests).toBe(1);
  });

  it("opens from the reading pane of the main window and reports a refusal", async () => {
    const { core, items } = setup();
    const { user } = renderApp();
    const list = await screen.findByRole("listbox", { name: /^Messages in/ });
    list.focus();
    await user.keyboard("{Enter}");
    await screen.findByRole("region", { name: "Message" });
    const toolbar = screen.getByRole("toolbar", { name: "Message actions" });
    await user.click(await toolbarAction(user, toolbar, "Open in new window"));
    expect(core.ui.messageWindows).toEqual([`${ACCOUNT.id}/${String(items[0]?.id)}`]);

    // The message vanished meanwhile: the core opens nothing, a notice says why.
    core.setMessages(1, items.slice(1));
    await user.click(await toolbarAction(user, toolbar, "Open in new window"));
    expect(await screen.findByText("The message could not be opened in a new window.")).toBeTruthy();
    expect(core.ui.messageWindows).toHaveLength(1);
  });
});

describe("the composer window", () => {
  it("edits one draft in the whole window and closes the window when the draft is closed", async () => {
    const { core } = setup({ canCloseWindow: true });
    openWindow(`?window=composer&draftId=${DRAFT_A}`);
    const { user } = renderApp();
    const composer = await screen.findByTestId("composer");
    expect(screen.getByLabelText<HTMLInputElement>("Subject").value).toBe("First draft");
    expect(core.callsOf("get_draft")).toEqual([{ draftId: DRAFT_A }]);
    expect(screen.queryByRole("navigation", { name: "Views" })).toBeNull();
    // Already in its own window: no second offer.
    expect(within(composer).queryByTestId("composer-open-window")).toBeNull();
    await user.click(screen.getByTestId("composer-close"));
    await waitFor(() => {
      expect(core.ui.closeRequests).toBe(1);
    });
    expect(core.callsOf("close_draft")).toEqual([{ draftId: DRAFT_A }]);
    expect(screen.queryByTestId("window-close-failed")).toBeNull();
  });

  it("closes after discarding, and says the work is done when the shell may not close it", async () => {
    const { core } = setup();
    openWindow(`?window=composer&draftId=${DRAFT_B}`);
    const { user } = renderApp();
    await screen.findByTestId("composer");
    await user.click(screen.getByTestId("composer-discard"));
    await user.click(within(await screen.findByTestId("composer-confirm-discard")).getByRole("button", { name: "Discard" }));
    expect(await screen.findByRole("heading", { name: "Done" })).toBeTruthy();
    expect(core.compose.deleted).toEqual([DRAFT_B]);
    expect(core.ui.closeRequests).toBe(1);
    expect((await screen.findByTestId("window-close-failed")).textContent).toContain("Close it with its close button.");
  });

  it("stays open while a send can still be taken back, then closes", async () => {
    const { core } = setup({ canCloseWindow: true });
    openWindow(`?window=composer&draftId=${DRAFT_A}`);
    const { user } = renderApp();
    await screen.findByTestId("composer");
    await user.click(screen.getByTestId("composer-send"));
    expect(await screen.findByRole("heading", { name: "Sending" })).toBeTruthy();
    expect(core.ui.closeRequests).toBe(0);
    const [mail] = core.compose.outgoing;
    if (!mail) throw new Error("no send");
    await emitCore({ type: "outgoingChanged", accountId: ACCOUNT.id, mails: [core.compose.setOutgoing(mail.operationId, { state: "sending" })] });
    await waitFor(() => {
      expect(core.ui.closeRequests).toBe(1);
    });
  });

  it("brings the draft back into the same window with Undo", async () => {
    const { core } = setup({ canCloseWindow: true });
    openWindow(`?window=composer&draftId=${DRAFT_A}`);
    const { user } = renderApp();
    await screen.findByTestId("composer");
    await user.click(screen.getByTestId("composer-send"));
    await screen.findByRole("heading", { name: "Sending" });
    await user.click(await screen.findByRole("button", { name: "Undo" }));
    expect(await screen.findByTestId("composer")).toBeTruthy();
    expect(core.ui.closeRequests).toBe(0);
  });

  it("opens drafts of the main window in their own windows, several at once", async () => {
    const { core } = setup();
    const { user } = renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    act(() => {
      openComposer({ kind: "draft", draftId: DRAFT_A });
    });
    await screen.findByLabelText("Subject");
    await user.type(screen.getByLabelText("Subject"), " changed");
    await user.click(screen.getByTestId("composer-open-window"));
    await waitFor(() => {
      expect(screen.queryByTestId("composer")).toBeNull();
    });
    // Saved first, so the new window loads the latest text; the window's composer hands it on later.
    expect(core.compose.drafts.get(DRAFT_A)?.subject).toBe("First draft changed");
    expect(core.callsOf("close_draft")).toHaveLength(0);

    act(() => {
      openComposer({ kind: "draft", draftId: DRAFT_B });
    });
    await screen.findByLabelText("Subject");
    await user.click(screen.getByTestId("composer-open-window"));
    await waitFor(() => {
      expect(core.ui.composerWindows).toEqual([DRAFT_A, DRAFT_B]);
    });
  });

  it("keeps the composer and says why when the window cannot open", async () => {
    setup({ failures: { open_composer_window: ipcError("internal", "window builder failed") } });
    const { user } = renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    act(() => {
      openComposer({ kind: "draft", draftId: DRAFT_A });
    });
    await screen.findByLabelText("Subject");
    await user.click(screen.getByTestId("composer-open-window"));
    expect(await screen.findByText("The draft could not be opened in its own window.")).toBeTruthy();
    expect(screen.getByTestId("composer")).toBeTruthy();
    expect(useComposerStore.getState().session).not.toBeNull();
  });
});

describe("Settings → Writing: default quote position", () => {
  it("shows the core's default and saves a change together with the undo delay", async () => {
    const core = mockCore({ accounts: [ACCOUNT], compose: { settings: { sendDelaySeconds: 10, defaultQuotePosition: "above" } } });
    const { user } = renderApp();
    act(() => {
      useUiStore.getState().showSettings("compose");
    });
    const select = await screen.findByRole<HTMLSelectElement>("combobox", { name: "Answer in replies and forwards" });
    await waitFor(() => {
      expect(select.disabled).toBe(false);
    });
    expect(select.value).toBe("above");
    expect(within(select).getAllByRole("option").map((o) => o.textContent)).toEqual(["Above the quoted message", "Below the quoted message"]);
    await user.selectOptions(select, "below");
    await waitFor(() => {
      expect(core.callsOf("set_compose_settings")).toEqual([{ settings: { sendDelaySeconds: 10, defaultQuotePosition: "below" } }]);
    });
    await user.selectOptions(screen.getByTestId("send-delay"), "30");
    await waitFor(() => {
      expect(core.callsOf("set_compose_settings").at(-1)).toEqual({ settings: { sendDelaySeconds: 30, defaultQuotePosition: "below" } });
    });
  });
});

