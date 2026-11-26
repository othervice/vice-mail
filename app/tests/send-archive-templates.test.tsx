import { act, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DraftUpdate, MessageListItem, TemplateInput } from "../src/services/ipc";
import { useComposerStore } from "../src/stores/composer";
import { useUiStore } from "../src/stores/ui";
import { makeDraft, makeIdentity, makeOutgoing, type ComposeFakeOptions } from "./support/compose";
import { ipcError, makeAccount, makeMessages, makeView, mockCore, standardMailboxes, type Failure } from "./support/core";
import { renderApp } from "./support/render";
import { makeTemplate, type SnippetsFakeOptions } from "./support/snippets";
import { menuEntries, menuEntry, openComposerMenu } from "./support/composerMenus";

// Stage 16: "Send and archive" and "Request a read receipt" in the composer, the notice when no
// Archive folder exists, and templates that prefill recipients.

const ADA = makeAccount({ writesEnabled: true });
const GRACE = { name: "Grace Hopper", email: "grace@example.org" };

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  useComposerStore.setState({ session: null, openDraftId: null, sends: [], problems: [] });
});

interface SetupOptions {
  compose?: ComposeFakeOptions;
  snippets?: SnippetsFakeOptions;
  failures?: Partial<Record<string, Failure>>;
}

function setup({ compose = {}, snippets = {}, failures }: SetupOptions = {}) {
  const items = makeMessages(2);
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    compose: { identities: { [ADA.id]: [makeIdentity({ accountId: ADA.id })] }, ...compose },
    snippets,
    ...(failures && { failures }),
  });
  return { core, first: items[0] as MessageListItem, ...renderApp() };
}

function replyDraft(overrides: Parameters<typeof makeDraft>[0] = {}) {
  return makeDraft({ accountId: ADA.id, kind: "reply", to: [GRACE], subject: "Re: Hello", bodyText: "Thanks", originalMessageId: 42, ...overrides });
}

async function openDraft(draft: ReturnType<typeof makeDraft>) {
  await screen.findByRole("button", { name: "New message" });
  act(() => {
    useComposerStore.getState().open({ kind: "draft", draftId: draft.id, draft });
  });
  await screen.findByLabelText("Subject");
  return screen.getByTestId("composer");
}

const order = (core: ReturnType<typeof mockCore>, ...cmds: string[]) =>
  core.calls.map((c) => c.cmd).filter((cmd) => cmds.includes(cmd));

describe("composer: send and archive", () => {
  it("the menu next to Send turns on archiving the original, then sends with the undo delay", async () => {
    const draft = replyDraft();
    const { core, user } = setup({ compose: { drafts: [draft] } });
    const composer = await openDraft(draft);
    await user.click(within(composer).getByRole("button", { name: "More ways to send" }));
    const item = within(composer).getByRole("menuitem", { name: /Send and archive/ });
    expect(item.getAttribute("aria-keyshortcuts")).toBe("Meta+Shift+Enter");
    await user.click(item);
    await waitFor(() => {
      expect(core.callsOf("send_draft")).toHaveLength(1);
    });
    expect(core.callsOf("set_draft_send_options")).toEqual([
      { draftId: draft.id, options: { archiveOriginal: true, requestReadReceipt: false, followUpAt: null } },
    ]);
    expect(order(core, "set_draft_send_options", "send_draft")).toEqual(["set_draft_send_options", "send_draft"]);
    // The usual undo notice; the send waits for archiving until the server accepted it.
    const notice = await screen.findByTestId("send-notice");
    expect(notice.textContent).toMatch(/Sending in (10|9) seconds…/);
    expect(core.compose.outgoing[0]?.archive).toBe("waiting");
  });

  it("Cmd+Shift+Enter is “Send and archive”", async () => {
    const draft = replyDraft();
    const { core, user } = setup({ compose: { drafts: [draft] } });
    await openDraft(draft);
    await user.click(screen.getByLabelText("Subject"));
    await user.keyboard("{Meta>}{Shift>}{Enter}{/Shift}{/Meta}");
    await waitFor(() => {
      expect(core.callsOf("send_draft")).toHaveLength(1);
    });
    expect(core.callsOf("set_draft_send_options")[0]?.["options"]).toEqual({ archiveOriginal: true, requestReadReceipt: false, followUpAt: null });
  });

  it("Cmd+Enter alone sends without touching the send options", async () => {
    const draft = replyDraft();
    const { core, user } = setup({ compose: { drafts: [draft] } });
    await openDraft(draft);
    await user.click(screen.getByLabelText("Subject"));
    await user.keyboard("{Meta>}{Enter}{/Meta}");
    await waitFor(() => {
      expect(core.callsOf("send_draft")).toHaveLength(1);
    });
    expect(core.callsOf("set_draft_send_options")).toEqual([]);
    expect(core.compose.outgoing[0]?.archive).toBeNull();
  });

  it("a new message has nothing to archive: no option, no menu entry, Cmd+Shift+Enter does nothing", async () => {
    const draft = makeDraft({ accountId: ADA.id, to: [GRACE], subject: "Hi", bodyText: "Hello" });
    const { core, user } = setup({ compose: { drafts: [draft] } });
    await openDraft(draft);
    // Stage 23 (decision 0086): the arrow of the send button stays (it has "Send later").
    const options = await openComposerMenu(user, "composer-options");
    expect(menuEntries(options).some((entry) => entry.textContent === "Archive the original after sending")).toBe(false);
    await user.keyboard("{Escape}");
    const later = await openComposerMenu(user, "composer-send-later");
    expect(menuEntries(later).some((entry) => /Send and archive/.test(entry.textContent ?? ""))).toBe(false);
    await user.keyboard("{Escape}");
    await user.click(screen.getByLabelText("Subject"));
    await user.keyboard("{Meta>}{Shift>}{Enter}{/Shift}{/Meta}");
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(core.callsOf("send_draft")).toEqual([]);
  });

  it("a reply starts with the setting “Archive the original when I reply”", async () => {
    const draft = replyDraft({ archiveOriginal: true });
    const { user } = setup({ compose: { drafts: [draft], settings: { sendDelaySeconds: 10, archiveOnReply: true } } });
    await openDraft(draft);
    const menu = await openComposerMenu(user, "composer-options");
    const box = menuEntry(menu, "Archive the original after sending");
    expect(box.getAttribute("role")).toBe("menuitemcheckbox");
    expect(box.getAttribute("aria-checked")).toBe("true");
    await user.click(box);
    // An option keeps the menu open for the next one.
    expect(menuEntry(menu, "Archive the original after sending").getAttribute("aria-checked")).toBe("false");
  });

  it("“Request a read receipt” is stored on the draft; a refusal puts the switch back and says why", async () => {
    const draft = replyDraft();
    const { core, user } = setup({ compose: { drafts: [draft] } });
    await openDraft(draft);
    const menu = await openComposerMenu(user, "composer-options");
    const box = menuEntry(menu, "Request a read receipt");
    // The hint says what it is, as the item's description.
    expect(document.getElementById(box.getAttribute("aria-describedby") ?? "")?.textContent).toContain("Nothing is tracked.");
    await user.click(box);
    await waitFor(() => {
      expect(core.callsOf("set_draft_send_options")).toEqual([
        { draftId: draft.id, options: { archiveOriginal: false, requestReadReceipt: true, followUpAt: null } },
      ]);
    });
    expect(core.compose.drafts.get(draft.id)?.requestReadReceipt).toBe(true);
    expect(menuEntry(menu, "Request a read receipt").getAttribute("aria-checked")).toBe("true");
  });

  it("a refused send option is taken back with the reason", async () => {
    const draft = replyDraft();
    const { user } = setup({ compose: { drafts: [draft] }, failures: { set_draft_send_options: ipcError("send_in_progress") } });
    await openDraft(draft);
    const menu = await openComposerMenu(user, "composer-options");
    await user.click(menuEntry(menu, "Request a read receipt"));
    const error = await screen.findByTestId("composer-action-error");
    expect(error.textContent).toContain("The send options could not be changed.");
    expect(menuEntry(await openComposerMenu(user, "composer-options"), "Request a read receipt").getAttribute("aria-checked")).toBe("false");
  });

  it("says quietly when the account had no Archive folder for the original", async () => {
    setup();
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useComposerStore.getState().track(makeOutgoing({ state: "sent", archive: "no_archive_mailbox" }));
    });
    const notice = await screen.findByTestId("send-notice");
    expect(notice.textContent).toContain("The original was not archived: this account has no Archive folder yet.");
  });

  it("a read receipt in the outbox says what it is instead of a subject", async () => {
    setup();
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useComposerStore.getState().track(makeOutgoing({ state: "queued", readReceipt: true, subject: "" }));
    });
    expect((await screen.findByTestId("send-notice")).textContent).toContain("Read receipt");
  });
});

describe("templates with recipients", () => {
  const saves = (core: ReturnType<typeof mockCore>) => core.callsOf("save_template").map((c) => c["template"] as TemplateInput);

  async function openWriting(options: SetupOptions = {}) {
    const rendered = setup(options);
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useUiStore.getState().showSettings("compose");
    });
    await screen.findByRole("heading", { name: "Templates" });
    return rendered;
  }

  it("the editor stores To, Cc and Bcc; empty fields are left out", async () => {
    const { core, user } = await openWriting();
    await user.click(await screen.findByRole("button", { name: "New template" }));
    const editor = await screen.findByTestId("template-editor");
    await user.type(within(editor).getByLabelText("Name"), "Team update");
    const recipients = within(editor).getByRole("group", { name: "Recipients" });
    await user.type(within(recipients).getByRole("combobox", { name: "To" }), "team@example.org,");
    await user.type(within(recipients).getByRole("combobox", { name: "Bcc" }), "Archive <archive@example.org>");
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(saves(core)).toHaveLength(1);
    });
    const saved = saves(core)[0];
    expect(saved?.to).toEqual([{ name: null, email: "team@example.org" }]);
    expect(saved?.bcc).toEqual([{ name: "Archive", email: "archive@example.org" }]);
    expect(saved && "cc" in saved).toBe(false);
    expect(core.snippets.templates[0]?.to).toEqual([{ name: null, email: "team@example.org" }]);
  });

  it("an address the core refuses is marked and the save error says why", async () => {
    const template = makeTemplate({ to: [{ name: null, email: "bad@@example" }] });
    const { user } = await openWriting({ snippets: { templates: [template] } });
    await user.click(await screen.findByRole("button", { name: "Edit template Thank you" }));
    const editor = await screen.findByTestId("template-editor");
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    const error = await within(editor).findByTestId("template-error");
    expect(error.getAttribute("data-error-code")).toBe("invalid_address");
    const chip = within(within(editor).getByTestId("template-to")).getByTestId("recipient-chip");
    expect(chip.getAttribute("data-invalid")).toBe("true");
  });

  it("inserting a template adds its recipients after the draft's own, without doubles", async () => {
    const template = makeTemplate({
      id: "tpl-team",
      name: "Team",
      html: "<p>Hello team</p>",
      to: [GRACE, { name: null, email: "team@example.org" }],
      cc: [{ name: null, email: "lead@example.org" }],
    });
    const draft = makeDraft({ accountId: ADA.id, to: [GRACE], subject: "", bodyText: "" });
    const { core, user } = setup({ compose: { drafts: [draft] }, snippets: { templates: [template] } });
    await openDraft(draft);
    await user.click(screen.getByRole("button", { name: "Insert template" }));
    const menu = await screen.findByRole("group", { name: "Templates" });
    await user.click(await within(menu).findByRole("button", { name: /Team/ }));
    const note = await screen.findByTestId("composer-template-recipients");
    expect(note.textContent).toBe("It added 2 recipients.");
    const to = within(screen.getByTestId("composer-to")).getAllByTestId("recipient-chip").map((c) => c.getAttribute("title"));
    expect(to).toEqual(["Grace Hopper <grace@example.org>", "team@example.org"]);
    expect(screen.getByRole("combobox", { name: "Cc" })).toBeTruthy();
    await user.click(screen.getByLabelText("Subject"));
    await user.tab();
    await waitFor(() => {
      const last = core.callsOf("save_draft").at(-1)?.["update"] as DraftUpdate | undefined;
      expect(last?.cc).toEqual([{ name: null, email: "lead@example.org" }]);
    });
  });
});
