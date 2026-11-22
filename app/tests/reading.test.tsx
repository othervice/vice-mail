import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MAIL_FRAME_SANDBOX } from "../src/features/reading/MessageBodyView";
import { formatFullDate } from "../src/features/messages/rows";
import type { BodyFormat, LinkInfo, MessageListItem, MessageView } from "../src/services/ipc";
import {
  htmlBody,
  ipcError,
  makeAccount,
  makeAttachment,
  makeMessage,
  makeView,
  mockCore,
  renderToken,
  standardMailboxes,
  textBody,
} from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

const ACCOUNT = makeAccount();

/** The list virtualizer needs a viewport height (jsdom has no layout). */
beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

function grace(overrides: Partial<MessageListItem> = {}): MessageListItem {
  return makeMessage({
    subject: "Compiler notes",
    from: { name: "Grace Hopper", email: "grace@example.org" },
    to: [{ name: "Ada Lovelace", email: "ada@vicemail.test" }],
    ...overrides,
  });
}

type Views = Record<number, MessageView | ((format: BodyFormat) => MessageView)>;

function inbox(items: MessageListItem[], views: Views, extra: Parameters<typeof mockCore>[0] = {}) {
  return mockCore({ accounts: [ACCOUNT], mailboxes: standardMailboxes(), messages: { 1: items }, views, ...extra });
}

async function openMessage(user: ReturnType<typeof renderApp>["user"], index = 0) {
  const options = await screen.findAllByRole("option");
  const option = options[index];
  if (!option) throw new Error(`no row ${String(index)}`);
  await user.click(option);
  return screen.findByRole("region", { name: "Message" });
}

function link(overrides: Partial<LinkInfo> = {}): LinkInfo {
  return {
    target: "https://example.org/docs",
    scheme: "https",
    host: "example.org",
    text: "the docs",
    warnings: [],
    openable: true,
    ...overrides,
  };
}

/** Commands that only read. Opening a message must not change anything (stage 4 marks read). */
const READING_COMMANDS = new Set([
  "get_app_info",
  "get_preferences",
  "list_accounts",
  "list_mailboxes",
  "list_messages",
  "get_message",
  "plugin:event|listen",
  "plugin:event|unlisten",
]);

describe("reading pane", () => {
  it("opens next to the list with head and the HTML in a sandboxed frame, srcdoc unchanged", async () => {
    const message = grace({ date: new Date(2026, 10, 20, 14, 30).getTime() });
    const view = makeView(message, { body: htmlBody("<p>Hi Ada, <a href='https://link.invalid/x/0'>notes</a></p>") });
    const core = inbox([message], { [message.id]: view });
    const { user } = renderApp();

    const pane = await openMessage(user);

    expect(within(pane).getByRole("heading", { level: 2, name: "Compiler notes" })).toBeTruthy();
    expect(within(pane).getByRole("article", { name: "Compiler notes" })).toBeTruthy();
    expect(within(pane).getByTestId("reading-from").textContent).toBe("From: Grace Hopper grace@example.org");
    expect(within(pane).getByText("Ada Lovelace")).toBeTruthy();
    expect(within(pane).getByTestId("reading-date").textContent).toBe(formatFullDate(message.date, "en"));
    // The account: its colour together with its name.
    const accountLine = within(pane).getByTestId("reading-account");
    expect(accountLine.textContent).toBe("Ada Lovelace (ada@vicemail.test)");
    expect(accountLine.querySelector(".bg-account-1")).toBeTruthy();

    const frame = within(pane).getByTitle("Content of the message “Compiler notes”");
    expect(frame.tagName).toBe("IFRAME");
    expect(frame.getAttribute("sandbox")).toBe("allow-same-origin");
    expect(frame.getAttribute("srcdoc")).toBe(view.body.kind === "html" ? view.body.srcdoc : "");
    // Its own white canvas, in the light scheme, in both themes.
    expect(frame.className).toContain("bg-mail-canvas");
    expect(frame.className).toContain("scheme-light");

    expect(core.callsOf("get_message")).toEqual([{ messageId: message.id, format: "auto" }]);
    expect(core.calls.map((c) => c.cmd).filter((cmd) => !READING_COMMANDS.has(cmd))).toEqual([]);
  });

  it("never gives a mail frame another sandbox value", async () => {
    const message = grace();
    inbox([message], { [message.id]: makeView(message) });
    const { user } = renderApp();
    await openMessage(user);

    expect(MAIL_FRAME_SANDBOX).toBe("allow-same-origin");
    const frames = [...document.querySelectorAll("iframe")];
    expect(frames).toHaveLength(1);
    for (const frame of frames) expect(frame.getAttribute("sandbox")).toBe("allow-same-origin");

    // In the source, the frame is the only one, and it takes the constant: no literal tokens.
    const sources = import.meta.glob("../src/**/*.tsx", { query: "?raw", import: "default", eager: true });
    const sandboxes = Object.entries(sources).flatMap(([file, source]) =>
      [...String(source).matchAll(/\bsandbox=\{?["'`]?([^"'`}\s]*)/g)].map((m) => `${file}: ${m[1] ?? ""}`),
    );
    expect(sandboxes).toEqual(["../src/features/reading/MessageBodyView.tsx: MAIL_FRAME_SANDBOX"]);
    const srcdocs = Object.entries(sources).filter(([, source]) => /\bsrcDoc=/.test(String(source)));
    expect(srcdocs.map(([file]) => file)).toEqual(["../src/features/reading/MessageBodyView.tsx"]);
  });

  it("makes the list narrower and its rows two lines while a message is open", async () => {
    const observers = installManualResizeObserver();
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(() =>
      document.querySelector('[data-testid="reading-pane"]') ? 384 : 1000,
    );
    const message = grace();
    inbox([message, grace({ subject: "Second" })], { [message.id]: makeView(message) });
    const { user } = renderApp();

    const list = await screen.findByRole("listbox");
    observers.fire();
    expect(within(list).getAllByRole("option")[0]?.className).toContain("h-(--ui-row-height)");
    expect(list.parentElement?.className).toContain("flex-1");

    await openMessage(user);
    observers.fire();

    await waitFor(() => {
      expect(within(list).getAllByRole("option")[0]?.className).toContain("h-(--ui-row-height-two-line)");
    });
    expect(list.parentElement?.className).toContain("w-(--ui-list-width-reading)");
  });

  it("shows a plain-text mail in the app's theme, as text, with links that go through the check", async () => {
    const message = grace();
    const token = renderToken();
    const view = makeView(message, {
      formats: ["text"],
      body: textBody(
        [
          { kind: "text", text: "See <b>this</b>:\n" },
          { kind: "link", text: "https://example.org/docs", index: 0 },
          { kind: "text", text: "\nBye" },
        ],
        token,
      ),
    });
    const core = inbox([message], { [message.id]: view }, { links: { [`${token}/0`]: link({ text: "https://example.org/docs" }) } });
    const { user } = renderApp();
    const pane = await openMessage(user);

    const text = within(pane).getByRole("document", { name: "Message text" });
    expect(text.textContent).toBe("See <b>this</b>:\nhttps://example.org/docs\nBye");
    expect(text.querySelector("b")).toBeNull();
    expect(within(pane).queryByTitle(/Content of the message/)).toBeNull();
    // Only one format: no switch.
    expect(within(pane).queryByRole("group", { name: "Show as" })).toBeNull();

    await user.click(within(text).getByRole("link", { name: "https://example.org/docs" }));

    const card = await within(pane).findByTestId("link-card");
    expect(await within(card).findByText("Open this link?")).toBeTruthy();
    expect(within(card).getByTestId("link-host").textContent).toBe("example.org");
    expect(within(card).getByTestId("link-target").textContent).toBe("https://example.org/docs");
    expect(core.callsOf("describe_link")).toEqual([{ renderToken: token, index: 0 }]);
    expect(core.callsOf("open_link")).toEqual([]);

    await user.click(within(card).getByRole("button", { name: "Open in browser" }));

    expect(core.callsOf("open_link")).toEqual([{ renderToken: token, index: 0 }]);
    await waitFor(() => {
      expect(within(pane).queryByTestId("link-card")).toBeNull();
    });
  });

  it("switches between HTML and plain text with a new request in the other format", async () => {
    const message = grace();
    const html = makeView(message);
    const text = makeView(message, { body: textBody([{ kind: "text", text: "Hello as text" }]) });
    const core = inbox([message], { [message.id]: (format) => (format === "text" ? text : html) });
    const { user } = renderApp();
    const pane = await openMessage(user);

    const toggle = within(pane).getByRole("group", { name: "Show as" });
    expect(within(toggle).getByRole("button", { name: "HTML" }).getAttribute("aria-pressed")).toBe("true");
    expect(within(toggle).getByRole("button", { name: "Plain text" }).getAttribute("aria-pressed")).toBe("false");

    await user.click(within(toggle).getByRole("button", { name: "Plain text" }));

    expect(await within(pane).findByText("Hello as text")).toBeTruthy();
    expect(within(pane).queryByTitle(/Content of the message/)).toBeNull();
    expect(within(toggle).getByRole("button", { name: "Plain text" }).getAttribute("aria-pressed")).toBe("true");
    expect(core.callsOf("get_message")).toEqual([
      { messageId: message.id, format: "auto" },
      { messageId: message.id, format: "text" },
    ]);

    await user.click(within(toggle).getByRole("button", { name: "HTML" }));
    expect(await within(pane).findByTitle("Content of the message “Compiler notes”")).toBeTruthy();
  });

  it("waits for messageBodyAvailable while the body is fetched, without asking again on its own", async () => {
    const message = grace();
    const core = inbox([message], { [message.id]: makeView(message, { body: { kind: "loading" } }) });
    const { user } = renderApp();
    const pane = await openMessage(user);

    // The head comes from the database at once; the body is on its way.
    expect(within(pane).getByRole("heading", { level: 2, name: "Compiler notes" })).toBeTruthy();
    expect(within(pane).getByRole("status").textContent).toBe("Loading message");
    expect(within(pane).getByRole("article").getAttribute("aria-busy")).toBe("true");
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(core.callsOf("get_message")).toHaveLength(1);

    // Another message's body does not concern this pane.
    await emitCore({ type: "messageBodyAvailable", accountId: ACCOUNT.id, messageId: message.id + 1 });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(core.callsOf("get_message")).toHaveLength(1);

    core.setView(message.id, makeView(message, { body: htmlBody("<p>Arrived</p>") }));
    await emitCore({ type: "messageBodyAvailable", accountId: ACCOUNT.id, messageId: message.id });

    expect(await within(pane).findByTitle("Content of the message “Compiler notes”")).toBeTruthy();
    expect(core.callsOf("get_message")).toHaveLength(2);
    expect(within(pane).getByRole("article").getAttribute("aria-busy")).toBe("false");
  });

  it("shows a failed fetch from messageBodyFailed with the error and a way to try again", async () => {
    const message = grace();
    const core = inbox([message], { [message.id]: makeView(message, { body: { kind: "loading" } }) });
    const { user } = renderApp();
    const pane = await openMessage(user);

    await emitCore({
      type: "messageBodyFailed",
      accountId: ACCOUNT.id,
      messageId: message.id,
      error: ipcError("connection_lost", "reset by peer"),
    });

    const alert = await within(pane).findByRole("alert");
    expect(alert.textContent).toContain("The message could not be downloaded.");
    expect(alert.textContent).toContain("The connection to the server was lost.");
    expect(core.callsOf("get_message")).toHaveLength(1);

    core.setView(message.id, makeView(message, { body: htmlBody("<p>Second try</p>") }));
    await user.click(within(alert).getByRole("button", { name: "Try again" }));

    expect(await within(pane).findByTitle("Content of the message “Compiler notes”")).toBeTruthy();
    expect(core.callsOf("get_message")).toHaveLength(2);
  });

  it("explains a message that is not on this computer and asks again once the account is online", async () => {
    const message = grace();
    const offline = makeAccount({
      status: { state: "offline", progress: null, error: ipcError("dns_failed"), retryAt: null, lastSyncAt: 1 },
    });
    const core = mockCore({
      accounts: [offline],
      mailboxes: standardMailboxes(),
      messages: { 1: [message] },
      views: { [message.id]: makeView(message, { body: { kind: "unavailable", reason: "offline", error: null } }) },
    });
    const { user } = renderApp();
    const pane = await openMessage(user);

    const notice = within(pane).getByTestId("reading-unavailable");
    expect(notice.textContent).toContain("This message is not on this computer yet");
    expect(notice.textContent).toContain("It downloads as soon as Ada Lovelace is online again.");
    expect(core.callsOf("get_message")).toHaveLength(1);

    core.setView(message.id, makeView(message, { body: { kind: "loading" } }));
    await emitCore({
      type: "accountStatusChanged",
      accountId: offline.id,
      status: { state: "idle", progress: null, error: null, retryAt: null, lastSyncAt: 2 },
    });

    await waitFor(() => {
      expect(core.callsOf("get_message")).toHaveLength(2);
    });
    expect(await within(pane).findByRole("status")).toBeTruthy();
  });

  it("says when a message is gone from the server, without a pointless retry", async () => {
    const message = grace();
    inbox([message], { [message.id]: makeView(message, { body: { kind: "unavailable", reason: "notOnServer", error: null } }) });
    const { user } = renderApp();
    const pane = await openMessage(user);

    const notice = within(pane).getByTestId("reading-unavailable");
    expect(notice.textContent).toContain("This message no longer exists on the server");
    expect(within(notice).queryByRole("button")).toBeNull();
  });

  it("shows an error with a retry when the core cannot open the message", async () => {
    const message = grace();
    const core = inbox([message], {});
    const { user } = renderApp();
    const pane = await openMessage(user);

    const alert = await within(pane).findByRole("alert");
    expect(alert.textContent).toContain("The message could not be opened.");
    expect(alert.getAttribute("data-error-code")).toBe("not_found");
    core.setView(message.id, makeView(message));
    await user.click(within(alert).getByRole("button", { name: "Try again" }));
    expect(await within(pane).findByTitle("Content of the message “Compiler notes”")).toBeTruthy();
  });

  it("points out damaged and partly stored messages", async () => {
    const message = grace();
    inbox([message], {
      [message.id]: makeView(message, { problems: ["brokenStructure", "unknownCharset"], partial: true }),
    });
    const { user } = renderApp();
    const pane = await openMessage(user);

    const problems = within(pane).getByTestId("reading-problems");
    expect(problems.textContent).toContain("This message may not be shown completely");
    expect(within(problems).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "Its structure is damaged. It is shown as far as it can be read.",
      "A part uses an unknown character set, so some characters may look wrong.",
    ]);
    expect(within(pane).getByTestId("reading-partial").textContent).toContain(
      "Only the text of this large message is on this computer",
    );
    // The readable part is still shown.
    expect(within(pane).getByTitle("Content of the message “Compiler notes”")).toBeTruthy();
  });

  it("does not reload the open message when its folder changes", async () => {
    const message = grace();
    const core = inbox([message], { [message.id]: makeView(message) });
    const { user } = renderApp();
    await openMessage(user);
    const before = core.callsOf("list_messages").length;

    await emitCore({ type: "messagesChanged", accountId: ACCOUNT.id, mailboxId: 1 });
    await emitCore({ type: "messageBodyAvailable", accountId: ACCOUNT.id, messageId: message.id });

    await waitFor(() => {
      expect(core.callsOf("list_messages").length).toBeGreaterThan(before);
    });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(core.callsOf("get_message")).toHaveLength(1);
  });
});

describe("external content", () => {
  function blockedView(message: MessageListItem) {
    return makeView(message, { body: htmlBody("<img src='data:,'>", { blockedRemote: 3 }) });
  }

  it("is blocked with a bar that loads it once", async () => {
    const message = grace();
    const loaded = makeView(message, { body: htmlBody("<img src='data:image/png;base64,AAAA'>", { remoteLoaded: true }) });
    const core = inbox([message], { [message.id]: blockedView(message) }, { remoteViews: { [message.id]: loaded } });
    const { user } = renderApp();
    const pane = await openMessage(user);

    const bar = within(pane).getByRole("group", { name: "3 external items blocked" });
    expect(bar.textContent).toContain("External images can tell the sender");

    await user.click(within(bar).getByRole("button", { name: "Load once" }));

    await waitFor(() => {
      expect(within(pane).queryByTestId("remote-content-bar")).toBeNull();
    });
    expect(core.callsOf("load_remote_content")).toEqual([{ messageId: message.id, scope: "once" }]);
    const frame = within(pane).getByTitle("Content of the message “Compiler notes”");
    expect(frame.getAttribute("srcdoc")).toBe(loaded.body.kind === "html" ? loaded.body.srcdoc : "");
    expect(core.remoteSenders(ACCOUNT.id)).toEqual([]);
  });

  it("loads it always for the sender, who then shows up in the settings and can be removed", async () => {
    const message = grace();
    const loaded = makeView(message, { body: htmlBody("<p>pictures</p>", { remoteLoaded: true }) });
    const core = inbox([message], { [message.id]: blockedView(message) }, { remoteViews: { [message.id]: loaded } });
    const { user } = renderApp();
    const pane = await openMessage(user);

    await user.click(within(pane).getByRole("button", { name: "Always load from grace@example.org" }));

    await waitFor(() => {
      expect(within(pane).queryByTestId("remote-content-bar")).toBeNull();
    });
    expect(core.callsOf("load_remote_content")).toEqual([{ messageId: message.id, scope: "sender" }]);

    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Reading" }));
    const senders = await screen.findByTestId(`remote-senders-${ACCOUNT.id}`);
    expect(await within(senders).findByText("grace@example.org")).toBeTruthy();

    await user.click(within(senders).getByRole("button", { name: "Block external content from grace@example.org again" }));

    expect(core.callsOf("revoke_remote_content")).toEqual([{ accountId: ACCOUNT.id, sender: "grace@example.org" }]);
    expect(await within(senders).findByText(/No senders/)).toBeTruthy();
  });

  it("shows why loading failed, and the bar stays", async () => {
    const message = grace();
    const core = inbox([message], { [message.id]: blockedView(message) }, { failures: { load_remote_content: ipcError("connection_timeout") } });
    const { user } = renderApp();
    const pane = await openMessage(user);

    await user.click(within(pane).getByRole("button", { name: "Load once" }));

    const alert = await within(pane).findByRole("alert");
    expect(alert.textContent).toContain("The external content could not be loaded.");
    expect(within(pane).getByTestId("remote-content-bar")).toBeTruthy();
    expect(core.callsOf("load_remote_content")).toHaveLength(1);
  });
});

describe("links", () => {
  async function htmlMessage(links: Record<number, LinkInfo>, extra: Parameters<typeof mockCore>[0] = {}) {
    const message = grace();
    const view = makeView(message);
    const token = view.body.kind === "html" ? view.body.renderToken : "";
    const core = inbox(
      [message],
      { [message.id]: view },
      { links: Object.fromEntries(Object.entries(links).map(([index, info]) => [`${token}/${index}`, info])), ...extra },
    );
    const app = renderApp();
    const pane = await openMessage(app.user);
    return { core, pane, token, message, user: app.user };
  }

  it("shows the real target of a click in the mail with warnings, and opens only after confirming", async () => {
    const { core, pane, token, user } = await htmlMessage({
      0: link({
        target: "https://xn--pple-43d.example/login",
        host: "аpple.example",
        text: "www.apple.com",
        warnings: [
          { kind: "domainMismatch", shownDomain: "apple.com", targetDomain: "аpple.example" },
          { kind: "confusableHost", asciiHost: "xn--pple-43d.example" },
        ],
      }),
    });

    // The core's navigation hook saw the click and sent the event.
    await emitCore({ type: "linkActivated", renderToken: token, index: 0 });

    const card = await within(pane).findByRole("group", { name: "Open this link?" });
    await waitFor(() => {
      expect(document.activeElement).toBe(card);
    });
    expect(within(card).getByText("Link text: “www.apple.com”")).toBeTruthy();
    const warnings = within(card).getAllByRole("listitem").map((li) => li.textContent);
    expect(warnings).toEqual([
      "The link text names apple.com, but the link leads to аpple.example.",
      "The address contains characters that look like other ones. Written plainly it is xn--pple-43d.example.",
    ]);
    // With warnings the safe answer is the prominent one.
    expect(within(card).getByRole("button", { name: "Open anyway" })).toBeTruthy();

    await user.click(within(card).getByRole("button", { name: "Cancel" }));

    expect(within(pane).queryByTestId("link-card")).toBeNull();
    expect(core.callsOf("open_link")).toEqual([]);
    expect(document.activeElement).toBe(within(pane).getByRole("article"));
  });

  it("ignores link events of renderings it does not show", async () => {
    const { core, pane } = await htmlMessage({ 0: link() });

    await emitCore({ type: "linkActivated", renderToken: "f".repeat(32), index: 0 });
    await new Promise((resolve) => setTimeout(resolve, 50));

    expect(within(pane).queryByTestId("link-card")).toBeNull();
    expect(core.callsOf("describe_link")).toEqual([]);
  });

  it("explains links that cannot be opened and offers no way to open them", async () => {
    const { core, pane, token, user } = await htmlMessage({
      1: link({ target: "vmlink:42", scheme: "vmlink", host: null, text: "click", warnings: [{ kind: "unsupportedScheme" }], openable: false }),
    });

    await emitCore({ type: "linkActivated", renderToken: token, index: 1 });

    const card = await within(pane).findByRole("group", { name: "This link cannot be opened" });
    expect(card.textContent).toContain("Links of the type “vmlink” cannot be opened from a message.");
    expect(within(card).queryByTestId("link-open")).toBeNull();
    await user.click(within(card).getByRole("button", { name: "Close" }));
    expect(within(pane).queryByTestId("link-card")).toBeNull();
    expect(core.callsOf("open_link")).toEqual([]);
  });

  it("hands mailto links to the mail app after confirming", async () => {
    const { core, pane, token, user } = await htmlMessage({
      2: link({ target: "mailto:grace@example.org", scheme: "mailto", host: null, text: "write me" }),
    });

    await emitCore({ type: "linkActivated", renderToken: token, index: 2 });

    const card = await within(pane).findByRole("group", { name: "Write to this address?" });
    expect(within(card).getByTestId("link-target").textContent).toBe("mailto:grace@example.org");
    await user.click(within(card).getByRole("button", { name: "Open in mail app" }));
    expect(core.callsOf("open_link")).toEqual([{ renderToken: token, index: 2 }]);
  });

  it("closes with Escape and returns to the message", async () => {
    const { pane, token, user } = await htmlMessage({ 0: link() });
    await emitCore({ type: "linkActivated", renderToken: token, index: 0 });
    await within(pane).findByRole("group", { name: "Open this link?" });

    await user.keyboard("{Escape}");

    expect(within(pane).queryByTestId("link-card")).toBeNull();
    expect(document.activeElement).toBe(within(pane).getByRole("article"));
  });

  it("offers to show the message again when the core forgot the rendering", async () => {
    const { core, pane, token, user } = await htmlMessage({});

    await emitCore({ type: "linkActivated", renderToken: token, index: 7 });

    const card = await within(pane).findByRole("group", { name: "This view of the message is out of date" });
    await user.click(within(card).getByRole("button", { name: "Show message again" }));

    await waitFor(() => {
      expect(core.callsOf("get_message")).toHaveLength(2);
    });
    expect(within(pane).queryByTestId("link-card")).toBeNull();
  });

  it("shows when opening failed", async () => {
    const { pane, token, user } = await htmlMessage({ 0: link() }, { failures: { open_link: ipcError("internal", "no browser") } });
    await emitCore({ type: "linkActivated", renderToken: token, index: 0 });
    const card = await within(pane).findByRole("group", { name: "Open this link?" });

    await user.click(within(card).getByRole("button", { name: "Open in browser" }));

    expect((await within(card).findByRole("alert")).textContent).toContain("The link could not be opened.");
    expect(within(pane).getByTestId("link-card")).toBeTruthy();
  });
});

describe("attachments", () => {
  const PDF = makeAttachment();
  const EXE = makeAttachment({ index: 1, name: "setup.exe", mimeType: "application/x-msdownload", size: 2048, executable: true });
  const VIDEO = makeAttachment({ index: 2, name: "talk.mp4", mimeType: "video/mp4", size: 31_457_280, local: false });

  async function withAttachments(extra: Parameters<typeof mockCore>[0] = {}, message = grace({ hasAttachments: true })) {
    const core = inbox([message], { [message.id]: makeView(message, { attachments: [PDF, EXE, VIDEO], partial: true }) }, extra);
    const app = renderApp();
    const pane = await openMessage(app.user);
    const region = within(pane).getByRole("region", { name: "3 attachments" });
    const items = within(region).getAllByRole("listitem");
    return { core, pane, region, items, message, user: app.user };
  }

  it("lists name, type and size in one line each; nothing opens by itself", async () => {
    const { core, region, items } = await withAttachments();

    expect(items.map((li) => within(li).getByTestId("attachment-name").textContent)).toEqual([
      "report.pdf",
      "setup.exe",
      "talk.mp4",
    ]);
    expect(items.map((li) => within(li).getByTestId("attachment-details").textContent)).toEqual([
      "PDF, 1.2 MB",
      "EXE, 2 kB",
      "MP4, about 30 MB",
    ]);
    expect(within(items[0] as HTMLElement).queryByText("Can run code")).toBeNull();
    expect(within(items[1] as HTMLElement).getByText("Can run code").className).toContain("text-warning");
    expect(within(items[2] as HTMLElement).getByText("Not downloaded yet")).toBeTruthy();
    expect(within(region).getByTestId("reading-partial").textContent).toBe(
      "Only the text of this large message is on this computer. Attachments download when you save or open them.",
    );
    expect(core.callsOf("open_attachment")).toEqual([]);
    expect(core.callsOf("save_attachment")).toEqual([]);
  });

  it("saves through the core's system dialog and says where", async () => {
    const { core, region, message, user } = await withAttachments();

    await user.click(within(region).getByRole("button", { name: "Save report.pdf" }));

    expect(core.callsOf("save_attachment")).toEqual([{ messageId: message.id, index: 0 }]);
    expect((await within(region).findByRole("status")).textContent).toBe(
      "report.pdf saved to /Users/ada/Downloads/report.pdf",
    );
  });

  it("says nothing when the save dialog was cancelled", async () => {
    const message = grace({ hasAttachments: true });
    const { core, region, user } = await withAttachments(
      { saveResults: { [`${String(message.id)}/0`]: { kind: "cancelled" } } },
      message,
    );
    await user.click(within(region).getByRole("button", { name: "Save report.pdf" }));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(core.callsOf("save_attachment")).toHaveLength(1);
    expect(within(region).queryByRole("status")).toBeNull();
    expect(within(region).queryByRole("alert")).toBeNull();
  });

  it("opens an ordinary file on click", async () => {
    const { core, region, message, user } = await withAttachments();
    await user.click(within(region).getByRole("button", { name: "Open report.pdf" }));
    expect(core.callsOf("open_attachment")).toEqual([{ messageId: message.id, index: 0, confirmedExecutable: false }]);
  });

  it("warns before opening a file that can run code and needs an explicit confirmation", async () => {
    const { core, region, message, user } = await withAttachments();

    await user.click(within(region).getByRole("button", { name: "Open setup.exe" }));

    const warning = within(region).getByRole("group", { name: "“setup.exe” can run code on your computer" });
    expect(document.activeElement).toBe(warning);
    expect(core.callsOf("open_attachment")).toEqual([]);

    await user.click(within(warning).getByRole("button", { name: "Cancel" }));
    expect(within(region).queryByTestId("attachment-warning")).toBeNull();
    expect(core.callsOf("open_attachment")).toEqual([]);

    await user.click(within(region).getByRole("button", { name: "Open setup.exe" }));
    await user.click(within(region).getByRole("button", { name: "Open anyway" }));

    expect(core.callsOf("open_attachment")).toEqual([{ messageId: message.id, index: 1, confirmedExecutable: true }]);
    await waitFor(() => {
      expect(within(region).queryByTestId("attachment-warning")).toBeNull();
    });
  });

  it("asks too when the core wants a confirmation for a file the list did not mark", async () => {
    const message = grace({ hasAttachments: true });
    const odd = makeAttachment({ index: 0, name: "invoice.pdf", mimeType: "application/pdf", executable: false });
    const core = inbox(
      [message],
      { [message.id]: makeView(message, { attachments: [odd] }) },
      { openResults: { [`${String(message.id)}/0`]: { kind: "needsConfirmation" } } },
    );
    const { user } = renderApp();
    const pane = await openMessage(user);
    const region = within(pane).getByRole("region", { name: "1 attachment" });

    await user.click(within(region).getByRole("button", { name: "Open invoice.pdf" }));

    const warning = await within(region).findByRole("group", { name: "“invoice.pdf” can run code on your computer" });
    await user.click(within(warning).getByRole("button", { name: "Open anyway" }));
    expect(core.callsOf("open_attachment")).toEqual([
      { messageId: message.id, index: 0, confirmedExecutable: false },
      { messageId: message.id, index: 0, confirmedExecutable: true },
    ]);
  });

  it("shows that an attachment of a large message is being downloaded", async () => {
    const { region, user } = await withAttachments({ hang: ["save_attachment"] });
    await user.click(within(region).getByRole("button", { name: "Save talk.mp4" }));
    const video = within(region).getAllByRole("listitem")[2] as HTMLElement;
    expect(within(video).getByRole("status").textContent).toBe("Downloading…");
    expect(within(region).getByRole("button", { name: "Open talk.mp4" }).hasAttribute("disabled")).toBe(true);
  });

  it("shows a failure under the list", async () => {
    const { region, user } = await withAttachments({ failures: { save_attachment: ipcError("database") } });
    await user.click(within(region).getByRole("button", { name: "Save report.pdf" }));
    const alert = await within(region).findByRole("alert");
    expect(alert.textContent).toContain("The attachment could not be saved.");
  });
});

describe("keyboard and focus", () => {
  it("Enter opens the message and moves the focus into it, Escape goes back, Escape again closes", async () => {
    const message = grace();
    inbox([message, grace({ subject: "Second" })], { [message.id]: makeView(message) });
    const { user } = renderApp();
    const list = await screen.findByRole("listbox");

    act(() => {
      list.focus();
    });
    // Focus selects the first message, which opens it next to the list.
    const pane = await screen.findByRole("region", { name: "Message" });
    expect(document.activeElement).toBe(list);

    await user.keyboard("{Enter}");
    const article = within(pane).getByRole("article");
    await waitFor(() => {
      expect(document.activeElement).toBe(article);
    });

    await user.keyboard("{Escape}");
    expect(document.activeElement).toBe(list);
    expect(screen.getByRole("region", { name: "Message" })).toBeTruthy();

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("region", { name: "Message" })).toBeNull();
    expect(document.activeElement).toBe(list);
  });

  it("a double click opens and focuses; arrow keys only change what the pane shows", async () => {
    const first = grace();
    const second = grace({ subject: "Second", date: first.date - 60_000, id: first.id - 60 });
    const core = inbox([first, second], { [first.id]: makeView(first), [second.id]: makeView(second) });
    const { user } = renderApp();
    const options = await screen.findAllByRole("option");

    await user.dblClick(options[0] as HTMLElement);
    const pane = await screen.findByRole("region", { name: "Message" });
    await waitFor(() => {
      expect(document.activeElement).toBe(within(pane).getByRole("article"));
    });

    act(() => {
      screen.getByRole("listbox").focus();
    });
    await user.keyboard("{ArrowDown}");
    expect(await screen.findByRole("heading", { level: 2, name: "Second" })).toBeTruthy();
    expect(document.activeElement).toBe(screen.getByRole("listbox"));
    expect(core.callsOf("get_message").map((c) => c["messageId"])).toEqual([first.id, second.id]);
  });

  it("the close button closes the pane and returns to the list", async () => {
    const message = grace();
    inbox([message], { [message.id]: makeView(message) });
    const { user } = renderApp();
    const pane = await openMessage(user);

    await user.click(within(pane).getByRole("button", { name: "Close message" }));

    expect(screen.queryByRole("region", { name: "Message" })).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole("listbox"));
    expect(screen.getByRole("option").getAttribute("aria-selected")).toBe("false");
  });

  it("recipient lists fold when long and unfold on request", async () => {
    const message = grace();
    const to = Array.from({ length: 6 }, (_, i) => ({ name: `Person ${String(i + 1)}`, email: `p${String(i + 1)}@example.org` }));
    inbox([message], { [message.id]: makeView(message, { header: { ...makeView(message).header, to } }) });
    const { user } = renderApp();
    const pane = await openMessage(user);

    const more = within(pane).getByRole("button", { name: "and 3 more" });
    expect(more.getAttribute("aria-expanded")).toBe("false");
    expect(within(pane).queryByText("Person 4")).toBeNull();

    await user.click(more);

    expect(within(pane).getByText("Person 6")).toBeTruthy();
    expect(within(pane).getByRole("button", { name: "show fewer" }).getAttribute("aria-expanded")).toBe("true");
  });

  it("speaks German too", async () => {
    const message = grace();
    inbox(
      [message],
      { [message.id]: makeView(message, { body: htmlBody("<p>x</p>", { blockedRemote: 1 }) }) },
      { preferences: { theme: "system", language: "de" } },
    );
    const { user } = renderApp("de");
    await user.click(await screen.findByRole("option"));
    const pane = await screen.findByRole("region", { name: "Nachricht" });
    expect(within(pane).getByRole("group", { name: "1 externer Inhalt blockiert" })).toBeTruthy();
    expect(within(pane).getByRole("button", { name: "Für grace@example.org immer laden" })).toBeTruthy();
    expect(within(pane).getByTitle("Inhalt der Nachricht „Compiler notes“")).toBeTruthy();
  });
});

describe("frame height in the pane", () => {
  it("follows the content of the frame once it has loaded", async () => {
    const message = grace();
    inbox([message], { [message.id]: makeView(message) });
    const { user } = renderApp();
    const pane = await openMessage(user);
    const frame = within(pane).getByTitle<HTMLIFrameElement>("Content of the message “Compiler notes”");
    const root = frame.contentDocument?.documentElement;
    if (!root) throw new Error("jsdom gives same-origin frames a document");
    Object.defineProperty(root, "scrollHeight", { configurable: true, get: () => 1234 });

    act(() => {
      fireEvent.load(frame);
    });

    expect(frame.style.height).toBe("1234px");
  });
});

/** A ResizeObserver the test triggers: jsdom has no layout, so nothing would ever resize. */
function installManualResizeObserver() {
  const observers = new Set<{ callback: ResizeObserverCallback; targets: Set<Element>; self: ResizeObserver }>();
  class ManualResizeObserver implements ResizeObserver {
    private readonly entry: { callback: ResizeObserverCallback; targets: Set<Element>; self: ResizeObserver };
    constructor(callback: ResizeObserverCallback) {
      this.entry = { callback, targets: new Set(), self: this };
      observers.add(this.entry);
    }
    observe(target: Element): void {
      this.entry.targets.add(target);
    }
    unobserve(target: Element): void {
      this.entry.targets.delete(target);
    }
    disconnect(): void {
      this.entry.targets.clear();
      observers.delete(this.entry);
    }
  }
  window.ResizeObserver = ManualResizeObserver;
  return {
    fire: () => {
      act(() => {
        for (const { callback, targets, self } of [...observers]) {
          const entries = [...targets].map(
            (target) => ({ target, contentRect: { width: (target as HTMLElement).clientWidth } }) as unknown as ResizeObserverEntry,
          );
          if (entries.length > 0) callback(entries, self);
        }
      });
    },
  };
}
