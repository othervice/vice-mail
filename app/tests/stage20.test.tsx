// Stage 20: the attachment reminder before sending (decision 0058), "Remind me if no reply" with the
// view "Waiting for reply" and the block above the inbox (0059), the attachments view across the
// accounts and the mailbox quota (0060), and the trackers the reading pane counts with clean links
// (0058). With the project's axe check on the new views.
import { act, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AttachmentEntry, LinkInfo, MailboxQuota, MessageListItem } from "../src/services/ipc";
import { useComposerStore } from "../src/stores/composer";
import { useUiStore } from "../src/stores/ui";
import { FOLLOW_UP_TIMES } from "./support/comfort";
import { makeDraft, makeIdentity, type ComposeFakeOptions } from "./support/compose";
import { htmlBody, makeAccount, makeMessage, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  useComposerStore.setState({ session: null, openDraftId: null, sends: [], problems: [] });
});

const ADA = makeAccount({ writesEnabled: true });

async function seriousFindings(): Promise<string[]> {
  const results = await axe.run(document.body, {
    resultTypes: ["violations"],
    iframes: false,
    rules: { "color-contrast": { enabled: false } },
  });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}

type Options = NonNullable<Parameters<typeof mockCore>[0]>;

function setup(compose: ComposeFakeOptions = {}, extra: Options = {}) {
  const items = makeMessages(2);
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    compose: { identities: { [ADA.id]: [makeIdentity({ accountId: ADA.id })] }, ...compose },
    ...extra,
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

describe("stage 20, attachment reminder", () => {
  const draft = () =>
    makeDraft({ accountId: ADA.id, to: [{ name: "Grace", email: "grace@example.org" }], subject: "Report", bodyText: "I attached the report." });

  it("asks before sending when the text speaks of an attachment; Send anyway sends", async () => {
    const d = draft();
    const { core, user } = setup({ drafts: [d], attachmentReminder: { [d.id]: "attached" } });
    const composer = await openDraft(d);
    await user.click(within(composer).getByTestId("composer-send"));
    const dialog = await screen.findByRole("alertdialog", { name: "Send without an attachment?" });
    expect(dialog.textContent).toContain("Your message says “attached”, but no file is attached.");
    expect(core.callsOf("send_draft")).toHaveLength(0);
    expect(await seriousFindings()).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "Send anyway" }));
    await waitFor(() => {
      expect(core.callsOf("send_draft")).toHaveLength(1);
    });
    // Asked once, not again for the confirmed send.
    expect(core.callsOf("check_attachment_reminder")).toHaveLength(1);
  });

  it("Add attachment opens the file dialog and sends nothing; Keep writing goes back", async () => {
    const d = draft();
    const { core, user } = setup({ drafts: [d], attachmentReminder: { "*": "Anhang" } });
    const composer = await openDraft(d);
    await user.click(within(composer).getByTestId("composer-send"));
    let dialog = await screen.findByRole("alertdialog", { name: "Send without an attachment?" });
    await user.click(within(dialog).getByRole("button", { name: "Add attachment" }));
    await waitFor(() => {
      expect(core.callsOf("pick_attachments")).toHaveLength(1);
    });
    expect(core.callsOf("send_draft")).toHaveLength(0);
    await user.click(within(composer).getByTestId("composer-send"));
    dialog = await screen.findByRole("alertdialog", { name: "Send without an attachment?" });
    await user.click(within(dialog).getByRole("button", { name: "Keep writing" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(core.callsOf("send_draft")).toHaveLength(0);
  });

  it("without a reminder it sends at once", async () => {
    const d = draft();
    const { core, user } = setup({ drafts: [d] });
    const composer = await openDraft(d);
    await user.click(within(composer).getByTestId("composer-send"));
    await waitFor(() => {
      expect(core.callsOf("send_draft")).toHaveLength(1);
    });
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("is a switch under Settings → Writing", async () => {
    const { core, user } = setup({ settings: { sendDelaySeconds: 10, attachmentReminder: true } });
    act(() => {
      useUiStore.getState().showSettings("compose");
    });
    const reminder = await screen.findByRole("switch", { name: "Remind me of missing attachments" });
    expect(reminder.getAttribute("aria-checked")).toBe("true");
    await user.click(reminder);
    await waitFor(() => {
      expect(core.callsOf("set_compose_settings").at(-1)?.["settings"]).toMatchObject({ attachmentReminder: false });
    });
  });
});

describe("stage 20, remind me if no reply", () => {
  it("the composer stores the chosen time with the draft's send options", async () => {
    const d = makeDraft({ accountId: ADA.id, to: [{ name: "Grace", email: "grace@example.org" }], subject: "Offer" });
    const { core, user } = setup({ drafts: [d] });
    const composer = await openDraft(d);
    const choice = within(composer).getByRole("combobox", { name: "Remind me if no reply:" });
    // The presets come with their times from the core.
    await within(choice).findByRole("option", { name: /^In 3 days/ });
    await user.selectOptions(choice, "inThreeDays");
    await waitFor(() => {
      expect(core.callsOf("set_draft_send_options").at(-1)?.["options"]).toMatchObject({ followUpAt: FOLLOW_UP_TIMES.inThreeDays });
    });
    await user.selectOptions(choice, "none");
    await waitFor(() => {
      expect(core.callsOf("set_draft_send_options").at(-1)?.["options"]).toMatchObject({ followUpAt: null });
    });
    // A time of one's own.
    await user.selectOptions(choice, "custom");
    const custom = within(composer).getByTestId("composer-follow-up-custom");
    await user.type(custom, "2030-01-02T09:30");
    await user.click(within(composer).getByTestId("composer-follow-up-set"));
    await waitFor(() => {
      expect(core.callsOf("set_draft_send_options").at(-1)?.["options"]).toMatchObject({ followUpAt: new Date(2030, 0, 2, 9, 30).getTime() });
    });
  });

  it("a returned reminder stands above the inbox with “No reply since …”; the view lists the waiting ones", async () => {
    const sentAt = Date.UTC(2026, 10, 20, 9, 0);
    const sent = makeMessage({ subject: "Proposal", mailboxId: 2, followUp: { due: Date.UTC(2026, 10, 23, 7, 0), sentAt, returned: true } });
    const waiting = makeMessage({ subject: "Invoice", mailboxId: 2, followUp: { due: Date.UTC(2026, 10, 30, 7, 0), sentAt, returned: false } });
    const inboxItem = makeMessage({ subject: "Hello" });
    const core = mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: [inboxItem], 2: [sent, waiting] },
      readingPreferences: { markReadOnOpen: "never" },
      comfort: {
        inboxPages: {
          1: [
            {
              rows: [],
              nextCursor: null,
              top: { pinned: [], returned: [], priority: [], priorityTotal: 0, priorityUnread: 0, followUps: [sent] },
              categories: false,
              conversations: [],
            },
          ],
        },
      },
    });
    const { user } = renderApp();
    expect(await screen.findByTestId("inbox-block-followUps")).toBeTruthy();
    const mark = await screen.findByTestId("row-mark-noReply");
    expect(mark.textContent).toMatch(/^No reply since (Nov 20|20 Nov)$/);

    act(() => {
      useUiStore.getState().showMailView("waiting");
    });
    await waitFor(() => {
      expect(core.callsOf("list_follow_ups")).toHaveLength(1);
    });
    const list = await screen.findByTestId("message-list");
    expect(within(list).getAllByRole("option").map((o) => o.textContent)).toEqual(
      expect.arrayContaining([expect.stringContaining("Invoice"), expect.stringContaining("Proposal")]),
    );
    expect(await seriousFindings()).toEqual([]);
    void user;
  });

  it("the action sets a reminder through the picker and removes it again", async () => {
    const sent = makeMessage({ subject: "Question", mailboxId: 2, from: { name: "Ada", email: ADA.email } });
    const core = mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 2: [sent] },
      views: { [sent.id]: makeView(sent) },
      readingPreferences: { markReadOnOpen: "never" },
    });
    const { user } = renderApp();
    const sentBox = standardMailboxes().find((m) => m.role === "sent");
    if (!sentBox) throw new Error("no Sent");
    act(() => {
      useUiStore.getState().showMailbox(ADA.id, sentBox.id);
    });
    const [row] = await screen.findAllByRole("option");
    if (!row) throw new Error("no row");
    await user.click(row);
    const pane = await screen.findByRole("region", { name: "Message" });
    await user.click(within(screen.getByRole("toolbar", { name: "Message actions" })).getByRole("button", { name: "More actions" }));
    await user.click(await screen.findByRole("menuitem", { name: "Remind me if no reply…" }));
    const picker = await screen.findByTestId("follow-up-picker");
    expect(await seriousFindings()).toEqual([]);
    await user.click(within(picker).getByTestId("follow-up-nextWeek"));
    await waitFor(() => {
      expect(core.callsOf("set_follow_up")).toEqual([{ messageIds: [sent.id], choice: { kind: "nextWeek" } }]);
    });
    expect(core.comfort.followUps.get(sent.id)?.due).toBe(FOLLOW_UP_TIMES.nextWeek);
  });
});

function entry(overrides: Partial<AttachmentEntry>): AttachmentEntry {
  return {
    messageId: 1,
    accountId: ADA.id,
    mailboxId: 1,
    index: 0,
    name: "file.bin",
    mimeType: "application/octet-stream",
    kind: "other",
    size: 1024,
    local: true,
    preview: null,
    executable: false,
    from: { name: "Grace Hopper", email: "grace@example.org" },
    subject: "Files",
    date: Date.UTC(2026, 10, 20, 9, 0),
    ...overrides,
  };
}

describe("stage 20, attachments view", () => {
  const ENTRIES = [
    entry({ messageId: 3, name: "Statement 2026.pdf", mimeType: "application/pdf", kind: "pdf", preview: "pdf", subject: "Bank" }),
    entry({ messageId: 2, index: 1, name: "holiday.jpg", mimeType: "image/jpeg", kind: "image", preview: "image", local: false }),
    entry({ messageId: 2, index: 0, name: "budget.xlsx", kind: "spreadsheet", from: { name: "Bob", email: "bob@friends.example" } }),
  ];

  it("lists every attachment with type, size, sender and date, filters by type, name and sender", async () => {
    const { core, user } = setup({}, { attachments: { attachmentEntries: ENTRIES } });
    act(() => {
      useUiStore.getState().showMailView("attachments");
    });
    const view = await screen.findByTestId("attachments-view");
    await waitFor(() => {
      expect(within(view).getAllByTestId("attachment-entry-name").map((e) => e.textContent)).toEqual([
        "Statement 2026.pdf",
        "holiday.jpg",
        "budget.xlsx",
      ]);
    });
    const first = within(view).getAllByTestId("attachment-entry")[0];
    if (!first) throw new Error("no entry");
    expect(first.textContent).toContain("PDF");
    expect(first.textContent).toContain("Grace Hopper");
    expect(within(view).getAllByTestId("attachment-entry")[1]?.textContent).toContain("about");
    expect(await seriousFindings()).toEqual([]);

    await user.click(within(view).getByRole("button", { name: "Images" }));
    await waitFor(() => {
      expect(within(view).getAllByTestId("attachment-entry-name").map((e) => e.textContent)).toEqual(["holiday.jpg"]);
    });
    expect(core.callsOf("list_attachments").at(-1)?.["filter"]).toMatchObject({ kind: "image", scope: { kind: "all" } });
    await user.click(within(view).getByRole("button", { name: "All" }));
    await user.type(within(view).getByLabelText("File name"), "statement");
    await waitFor(() => {
      expect(within(view).getAllByTestId("attachment-entry-name").map((e) => e.textContent)).toEqual(["Statement 2026.pdf"]);
    });
    await user.clear(within(view).getByLabelText("File name"));
    await user.type(within(view).getByLabelText("Sender"), "bob");
    await waitFor(() => {
      expect(within(view).getAllByTestId("attachment-entry-name").map((e) => e.textContent)).toEqual(["budget.xlsx"]);
    });
    await user.selectOptions(within(view).getByLabelText("Account"), ADA.id);
    await waitFor(() => {
      expect(core.callsOf("list_attachments").at(-1)?.["filter"]).toMatchObject({ scope: { kind: "account", accountId: ADA.id }, sender: "bob" });
    });
  });

  it("saves through the reading pane's command and goes to the message", async () => {
    const { core, user } = setup({}, { attachments: { attachmentEntries: ENTRIES } });
    act(() => {
      useUiStore.getState().showMailView("attachments");
    });
    const view = await screen.findByTestId("attachments-view");
    const pdf = (await within(view).findAllByTestId("attachment-entry"))[0];
    if (!pdf) throw new Error("no entry");
    await user.click(within(pdf).getByRole("button", { name: "Save Statement 2026.pdf" }));
    await waitFor(() => {
      expect(core.callsOf("save_attachment")).toEqual([{ messageId: 3, index: 0 }]);
    });
    await user.click(within(pdf).getByRole("button", { name: "Go to the message of Statement 2026.pdf" }));
    expect(useUiStore.getState().view).toEqual({ kind: "mailbox", accountId: ADA.id, mailboxId: 1 });
  });

  it("says so when nothing is stored yet, and where that is set", async () => {
    setup({}, { attachments: { attachmentEntries: [] } });
    act(() => {
      useUiStore.getState().showMailView("attachments");
    });
    const empty = await screen.findByTestId("attachments-view-empty");
    expect(empty.textContent).toContain("No attachments yet");
    expect(screen.getByTestId("attachments-view").textContent).toContain("Settings → Reading → Download in advance");
  });
});

describe("stage 20, trackers", () => {
  const trackers = { pixels: 2, knownHosts: 1, parameters: 1, hosts: ["news.example", "us5.list-manage.com"] };

  it("the reading pane counts blocked trackers and explains them on request", async () => {
    const message = makeMessage({ subject: "Newsletter" });
    mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: [message] },
      views: { [message.id]: makeView(message, { body: htmlBody("<p>Offers</p>", { trackers }) }) },
      readingPreferences: { markReadOnOpen: "never" },
    });
    const { user } = renderApp();
    const [row] = await screen.findAllByRole("option");
    if (!row) throw new Error("no row");
    await user.click(row);
    const notice = await screen.findByTestId("tracker-notice");
    const toggle = within(notice).getByRole("button", { name: "4 trackers blocked" });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    await user.click(toggle);
    const details = within(notice).getByTestId("tracker-notice-details");
    expect(details.hidden).toBe(false);
    expect(details.textContent).toContain("2 tracking pixels");
    expect(details.textContent).toContain("1 image from a known tracking service");
    expect(details.textContent).toContain("1 image with tracking parameters");
    expect(details.textContent).toContain("From: news.example, us5.list-manage.com");
    expect(await seriousFindings()).toEqual([]);
  });

  it("no notice without trackers", async () => {
    const message = makeMessage({ subject: "Plain" });
    mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: [message] },
      views: { [message.id]: makeView(message, { body: htmlBody("<p>Hi</p>") }) },
      readingPreferences: { markReadOnOpen: "never" },
    });
    const { user } = renderApp();
    const [row] = await screen.findAllByRole("option");
    if (!row) throw new Error("no row");
    await user.click(row);
    await screen.findByTestId("mail-frame");
    expect(screen.queryByTestId("tracker-notice")).toBeNull();
  });

  it("the link card shows the clean target and what was removed", async () => {
    const message = makeMessage({ subject: "Shop" });
    const token = "0".repeat(31) + "9";
    const link: LinkInfo = {
      target: "https://shop.example/offer?id=7",
      scheme: "https",
      host: "shop.example",
      text: "Offer",
      warnings: [],
      openable: true,
      trackingRemoved: ["utm_source", "fbclid"],
    };
    mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: [message] },
      views: { [message.id]: makeView(message, { body: htmlBody("<a href='x'>Offer</a>", { renderToken: token }) }) },
      readingPreferences: { markReadOnOpen: "never" },
      links: { [`${token}/0`]: link },
    });
    const { user } = renderApp();
    const [row] = await screen.findAllByRole("option");
    if (!row) throw new Error("no row");
    await user.click(row);
    await screen.findByTestId("mail-frame");
    await emitCore({ type: "linkActivated", renderToken: token, index: 0 });
    const card = await screen.findByTestId("link-card");
    expect(within(card).getByTestId("link-target").textContent).toBe("https://shop.example/offer?id=7");
    expect(within(card).getByTestId("link-tracking-removed").textContent).toBe("Tracking parameters removed: utm_source, fbclid");
  });

  it("both protections are switches under Settings → Reading", async () => {
    const { core, user } = setup({}, { readingPreferences: { markReadOnOpen: "never", blockTrackers: true, cleanLinks: true } });
    act(() => {
      useUiStore.getState().showSettings("reading");
    });
    const block = await screen.findByRole("switch", { name: "Block trackers in messages" });
    const clean = screen.getByRole("switch", { name: "Remove tracking from links" });
    expect(block.getAttribute("aria-checked")).toBe("true");
    await user.click(clean);
    await waitFor(() => {
      expect(core.callsOf("set_reading_preferences").at(-1)?.["preferences"]).toMatchObject({ cleanLinks: false, blockTrackers: true });
    });
  });
});

describe("stage 20, mailbox quota", () => {
  const nearlyFull: MailboxQuota = { kind: "reported", usedBytes: 950 * 1024 * 1024, limitBytes: 1024 * 1024 * 1024, percent: 92, nearlyFull: true };

  function showAccount() {
    act(() => {
      useUiStore.getState().showSettings("accounts");
      useUiStore.setState({ accountsPage: { kind: "account", accountId: ADA.id } });
    });
  }

  it("account settings show used and free storage, the notice from 90 %, and the way to make room", async () => {
    const { user } = setup({}, { attachments: { quotas: { [ADA.id]: nearlyFull } } });
    showAccount();
    const quota = await screen.findByTestId("account-quota");
    expect(quota.dataset["percent"]).toBe("92");
    expect(quota.textContent).toContain("950 MB of 1 GB used (92 %)");
    const warning = within(quota).getByTestId("account-quota-warning");
    expect(warning.textContent).toContain("The mailbox is 92 % full");
    expect(await seriousFindings()).toEqual([]);
    await user.click(within(warning).getByRole("button", { name: "Find large messages" }));
    expect(useUiStore.getState().view).toEqual({ kind: "mail", mailView: "inbox" });
  });

  it("a server without quota says so", async () => {
    setup({}, { attachments: { quotas: { [ADA.id]: { kind: "notReported" } } } });
    showAccount();
    expect((await screen.findByTestId("account-quota-none")).textContent).toBe("The server does not report storage.");
  });

  it("a nearly full mailbox shows on the account in the column", async () => {
    setup(
      {},
      {
        attachments: { quotas: { [ADA.id]: nearlyFull } },
        ui: { layout: { readingPane: "right", density: "comfortable", sidebarWidth: 240, listWidth: 384, listHeight: 320, contactPanel: true, sidebarExpanded: true } },
      },
    );
    const mark = await screen.findByTestId(`account-quota-mark-${ADA.id}`);
    const row = mark.closest("[role='treeitem']");
    expect(row?.getAttribute("aria-label")).toContain("mailbox 92 % full");
  });
});

// Keep the unused helper type import honest for future cases.
export type { MessageListItem };
