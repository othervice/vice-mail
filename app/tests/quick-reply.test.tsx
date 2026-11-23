import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DraftUpdate, MessageListItem } from "../src/services/ipc";
import { useComposerStore } from "../src/stores/composer";
import type { ComposeFakeOptions } from "./support/compose";
import { makeAccount, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { optionName } from "./support/names";
import { renderApp } from "./support/render";

// Stage 16: the inline quick reply under the open message (plain text, the same drafts and the
// same outbox as the composer, "Expand" into the full composer with the same draft).

const ADA = makeAccount({ writesEnabled: true });

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  useComposerStore.setState({ session: null, openDraftId: null, sends: [], problems: [] });
});

interface SetupOptions {
  account?: typeof ADA;
  compose?: ComposeFakeOptions;
}

function setup({ account = ADA, compose }: SetupOptions = {}) {
  const items = makeMessages(2);
  const first = items[0] as MessageListItem;
  const core = mockCore({
    accounts: [account],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    compose: compose ?? {
      replies: {
        [first.id]: {
          accountId: account.id,
          subject: `Re: ${first.subject}`,
          to: first.from ? [first.from] : [],
          format: "html",
          quoteText: "On Monday, Sender 1 wrote:\n> Hello",
          quoteHtml: "<blockquote>Hello</blockquote>",
          quotePosition: "above",
        },
      },
    },
  });
  return { core, first, items, ...renderApp() };
}

const saves = (core: ReturnType<typeof mockCore>) => core.callsOf("save_draft").map((c) => c["update"] as DraftUpdate);

async function openMessage(user: UserEvent, subject: string) {
  const listbox = await screen.findByRole("listbox", { name: /^Messages in/ });
  const row = within(listbox)
    .getAllByRole("option")
    .find((o) => optionName(o).includes(subject));
  if (!row) throw new Error("no row");
  await user.click(row);
  return screen.findByRole("textbox", { name: /^Quick reply to/ });
}

describe("inline quick reply", () => {
  it("creates no draft before the first key, then one reply draft saved as plain text", async () => {
    const { core, user, first } = setup();
    const field = await openMessage(user, first.subject);
    expect(field.getAttribute("placeholder")).toBe("Reply to Sender 1…");
    await user.click(field);
    expect(core.callsOf("create_draft")).toHaveLength(0);

    await user.type(field, "Thanks, see you!");
    await user.tab();
    await waitFor(() => {
      expect(saves(core).at(-1)?.bodyText).toBe("Thanks, see you!");
    });
    expect(core.callsOf("create_draft")).toEqual([{ request: { kind: "reply", messageId: first.id, language: "en" } }]);
    // Plain text, the quote as the writing settings place it.
    expect(saves(core).at(-1)?.format).toBe("text");
    expect(saves(core).at(-1)?.includeQuote).toBe(true);
    expect(saves(core).at(-1)?.quotePosition).toBe("above");
  });

  it("Cmd+Enter sends through the outbox with the usual undo delay; Undo reopens the same draft", async () => {
    const { core, user, first } = setup();
    const field = await openMessage(user, first.subject);
    await user.type(field, "On my way");
    await user.keyboard("{Meta>}{Enter}{/Meta}");
    await waitFor(() => {
      expect(core.callsOf("send_draft")).toHaveLength(1);
    });
    expect(core.callsOf("create_draft")).toHaveLength(1);
    const draftId = [...core.compose.drafts.keys()][0];
    expect(core.callsOf("send_draft")[0]?.["draftId"]).toBe(draftId);
    // The last save before sending carries the text.
    expect(saves(core).at(-1)?.bodyText).toBe("On my way");
    const notice = await screen.findByTestId("send-notice");
    expect(notice.textContent).toMatch(/Sending in (10|9) seconds…/);
    // The field starts over, without a draft.
    await waitFor(() => {
      expect(screen.getByRole<HTMLTextAreaElement>("textbox", { name: /^Quick reply to/ }).value).toBe("");
    });
    await user.click(within(notice).getByRole("button", { name: "Undo" }));
    await waitFor(() => {
      expect(useComposerStore.getState().session?.source).toMatchObject({ kind: "draft", draftId });
    });
    expect(core.callsOf("cancel_send")).toEqual([{ draftId }]);
  });

  it("Expand opens the full composer with the same draft and the text written so far", async () => {
    const { core, user, first } = setup();
    const field = await openMessage(user, first.subject);
    await user.type(field, "Longer answer");
    await user.click(screen.getByRole("button", { name: "Open this reply in the full composer" }));
    const composer = await screen.findByTestId("composer");
    await waitFor(() => {
      expect(within(composer).getByLabelText<HTMLTextAreaElement>("Message").value).toBe("Longer answer");
    });
    expect(core.callsOf("create_draft")).toHaveLength(1);
    const draftId = [...core.compose.drafts.keys()][0];
    expect(useComposerStore.getState().session?.source).toMatchObject({ kind: "draft", draftId });
    expect(screen.getByRole<HTMLTextAreaElement>("textbox", { name: /^Quick reply to/ }).value).toBe("");
  });

  it("Escape leaves the field for the message; the reading pane stays open", async () => {
    const { user, first } = setup();
    const field = await openMessage(user, first.subject);
    await user.click(field);
    await user.keyboard("{Escape}");
    expect(document.activeElement?.tagName).toBe("ARTICLE");
    expect(screen.getByTestId("reading-pane")).toBeTruthy();
  });

  it("leaves the quote out when asked and saves that", async () => {
    const { core, user, first } = setup();
    const field = await openMessage(user, first.subject);
    await user.type(field, "Short");
    await user.click(screen.getByRole("checkbox", { name: "Quote the message" }));
    await user.click(field);
    await user.tab();
    await waitFor(() => {
      expect(saves(core).at(-1)?.includeQuote).toBe(false);
    });
  });

  it("a read-only account cannot send: the error says why and where to allow changes", async () => {
    const readOnly = makeAccount({ writesEnabled: false });
    const { user, first } = setup({ account: readOnly });
    const field = await openMessage(user, first.subject);
    await user.type(field, "Hello");
    await user.click(screen.getByRole("button", { name: /^Send reply/ }));
    const error = await screen.findByTestId("inline-reply-error");
    expect(error.textContent).toContain("The reply was not sent.");
    expect(error.getAttribute("data-error-code")).toBe("read_only");
    expect(within(error).getByRole("button", { name: "Allow changes…" })).toBeTruthy();
    // The text stays.
    expect(screen.getByRole<HTMLTextAreaElement>("textbox", { name: /^Quick reply to/ }).value).toBe("Hello");
  });

  it("Discard removes the draft; leaving the message keeps a written one as a draft", async () => {
    const { core, user, first, items } = setup();
    const field = await openMessage(user, first.subject);
    await user.type(field, "Not this");
    await user.click(screen.getByRole("button", { name: "Discard this reply" }));
    await waitFor(() => {
      expect(core.callsOf("delete_draft")).toHaveLength(1);
    });
    expect(screen.getByRole<HTMLTextAreaElement>("textbox", { name: /^Quick reply to/ }).value).toBe("");

    await user.type(screen.getByRole("textbox", { name: /^Quick reply to/ }), "Keep this");
    const second = items[1] as MessageListItem;
    const listbox = screen.getByRole("listbox", { name: /^Messages in/ });
    const row = within(listbox)
      .getAllByRole("option")
      .find((o) => optionName(o).includes(second.subject));
    if (!row) throw new Error("no row");
    await user.click(row);
    await waitFor(() => {
      expect(core.callsOf("close_draft")).toHaveLength(1);
    });
    expect(saves(core).at(-1)?.bodyText).toBe("Keep this");
  });
});

describe("quick reply after a refused start", () => {
  it("does not ask the core again with every key, only when the user retries", async () => {
    // No reply prepared: the fake core refuses `create_draft` with `not_found`.
    const { core, user, first } = setup({ compose: {} });
    const field = await openMessage(user, first.subject);
    await user.type(field, "Thanks");
    const error = await screen.findByTestId("inline-reply-error");
    expect(error.textContent).toContain("not_found");
    await user.type(field, " a lot");
    expect(core.callsOf("create_draft")).toHaveLength(1);
    await user.click(within(error).getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(core.callsOf("create_draft")).toHaveLength(2);
    });
  });
});
