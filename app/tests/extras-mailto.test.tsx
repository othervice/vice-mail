import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { prefillDraft } from "../src/features/composer/Composer";
import type { ComposeRequest, DraftUpdate } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { makeDraft } from "./support/compose";
import { ipcError, makeAccount, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { makeComposeRequest } from "./support/extras";
import { renderApp } from "./support/render";

const ADA = makeAccount({ writesEnabled: true });

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

type Options = NonNullable<Parameters<typeof mockCore>[0]>;

function setup(launchLinks: ComposeRequest[] = [], extra: Options = {}) {
  const items = makeMessages(2);
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    extras: { launchLinks },
    ...extra,
  });
  return { core, ...renderApp() };
}

/** The composer once its form is there (the loading frame before it is another element). */
async function composerForm(): Promise<HTMLElement> {
  await screen.findByLabelText("Subject", {}, { timeout: 3000 });
  return screen.getByTestId("composer");
}

const saves = (core: ReturnType<typeof mockCore>) => core.callsOf("save_draft").map((c) => c["update"] as DraftUpdate);
const chips = (testId: string) =>
  within(screen.getByTestId(testId))
    .queryAllByTestId("recipient-chip")
    .map((chip) => chip.textContent);

/** A link like `mailto:grace@example.org?cc=lin@…&bcc=boss@…&subject=Gr%C3%BC%C3%9Fe&body=…&from=…` after the core's parser. */
const FULL = makeComposeRequest({
  to: [{ name: null, email: "grace@example.org" }],
  cc: [{ name: null, email: "lin@example.net" }],
  bcc: [{ name: null, email: "boss@example.com" }],
  subject: "Grüße & Fragen",
  body: "Hallo Grace,\nzwei Zeilen.",
  inReplyTo: null,
  notices: [{ kind: "bcc_present" }, { kind: "header_ignored", name: "from" }],
});

describe("mailto: links", () => {
  it("a link the app was started with opens a draft with every field, and sends nothing", async () => {
    const { core } = setup([FULL]);
    const composer = await composerForm();
    await waitFor(() => {
      expect(within(composer).getByLabelText<HTMLInputElement>("Subject").value).toBe("Grüße & Fragen");
    });
    expect(core.callsOf("take_launch_links")).toHaveLength(1);
    expect(core.callsOf("create_draft")[0]?.["request"]).toEqual({
      kind: "new",
      accountId: ADA.id,
      identityId: null,
      to: [{ name: null, email: "grace@example.org" }],
    });
    expect(saves(core)[0]).toMatchObject({
      to: [{ name: null, email: "grace@example.org" }],
      cc: [{ name: null, email: "lin@example.net" }],
      bcc: [{ name: null, email: "boss@example.com" }],
      subject: "Grüße & Fragen",
      bodyText: "Hallo Grace,\nzwei Zeilen.",
    });
    expect(chips("composer-to")).toEqual([expect.stringContaining("grace@example.org")]);
    expect(chips("composer-cc")).toEqual([expect.stringContaining("lin@example.net")]);
    // Blind copies are visible before sending, never hidden behind "Add Bcc".
    expect(chips("composer-bcc")).toEqual([expect.stringContaining("boss@example.com")]);
    expect(within(composer).getByTestId<HTMLTextAreaElement>("composer-text-body").value).toBe("Hallo Grace,\nzwei Zeilen.");

    const notices = within(composer).getByTestId("mailto-notices");
    expect(within(notices).getAllByTestId("mailto-notice").map((n) => n.textContent)).toEqual([
      "The link adds blind copies (Bcc). Check them before you send.",
      "The link tried to set “from”. That was ignored.",
    ]);
    expect(core.callsOf("send_draft")).toEqual([]);
  });

  it("says that a reply reference of the link cannot be kept", async () => {
    setup([makeComposeRequest({ to: [{ name: null, email: "grace@example.org" }], inReplyTo: "abc@example.org" })]);
    const composer = await composerForm();
    expect((await within(composer).findByTestId("mailto-notices")).textContent).toContain(
      "The link answers another message. This draft goes out as a new message, without that reference.",
    );
  });

  it("a plain link with only an address needs no extra save and shows no hints", async () => {
    const { core } = setup([makeComposeRequest({ to: [{ name: null, email: "grace@example.org" }] })]);
    const composer = await composerForm();
    expect(core.callsOf("save_draft")).toEqual([]);
    expect(within(composer).queryByTestId("mailto-notices")).toBeNull();
  });

  it("a link while the app runs (composeRequested) opens the composer", async () => {
    const { core } = setup();
    await screen.findAllByRole("option");
    expect(screen.queryByTestId("composer")).toBeNull();
    await emitCore({ type: "composeRequested", request: makeComposeRequest({ to: [{ name: "Lin", email: "lin@example.net" }], subject: "Hi" }) });
    const composer = await composerForm();
    await waitFor(() => {
      expect(within(composer).getByLabelText<HTMLInputElement>("Subject").value).toBe("Hi");
    });
    expect(core.callsOf("take_launch_links")).toHaveLength(1);
  });

  it("does not replace a message being written: the link waits, “Open now” takes it, the draft is kept", async () => {
    const { core, user } = setup();
    await user.click(await screen.findByRole("button", { name: "New message" }));
    await screen.findByLabelText("Subject");
    await user.type(screen.getByLabelText("Subject"), "My own");

    await emitCore({ type: "composeRequested", request: makeComposeRequest({ to: [{ name: null, email: "lin@example.net" }], subject: "From a link" }) });
    const waiting = await screen.findByTestId("mailto-waiting");
    expect(waiting.getAttribute("role")).toBe("status");
    expect(waiting.textContent).toContain("An email link is waiting");
    expect(screen.getByLabelText<HTMLInputElement>("Subject").value).toBe("My own");

    await user.click(within(waiting).getByRole("button", { name: "Open now" }));
    await waitFor(() => {
      expect(screen.getByLabelText<HTMLInputElement>("Subject").value).toBe("From a link");
    });
    // The replaced draft was saved and handed to the server copy.
    await waitFor(() => {
      expect(saves(core).some((update) => update.subject === "My own")).toBe(true);
    });
    expect(core.callsOf("close_draft").length).toBeGreaterThan(0);
    expect(screen.queryByTestId("mailto-waiting")).toBeNull();
  });

  it("opens waiting links one after the other as the composer closes; “Discard” drops them", async () => {
    const { user } = setup([
      makeComposeRequest({ to: [{ name: null, email: "one@example.org" }], subject: "One" }),
      makeComposeRequest({ to: [{ name: null, email: "two@example.org" }], subject: "Two" }),
      makeComposeRequest({ to: [{ name: null, email: "three@example.org" }], subject: "Three" }),
    ]);
    await waitFor(() => {
      expect(screen.getByLabelText<HTMLInputElement>("Subject").value).toBe("One");
    });
    expect((await screen.findByTestId("mailto-waiting")).textContent).toContain("2 email links are waiting");

    await user.click(screen.getByTestId("composer-close"));
    await waitFor(() => {
      expect(screen.getByLabelText<HTMLInputElement>("Subject").value).toBe("Two");
    });
    await user.click(within(screen.getByTestId("mailto-waiting")).getByRole("button", { name: "Discard" }));
    expect(screen.queryByTestId("mailto-waiting")).toBeNull();
    await user.click(screen.getByTestId("composer-close"));
    await waitFor(() => {
      expect(screen.queryByTestId("composer")).toBeNull();
    });
  });

  it("without an account the link waits with a way to add one, and opens once there is one", async () => {
    const { core, user } = setup([makeComposeRequest({ to: [{ name: null, email: "grace@example.org" }], subject: "Later" })], { accounts: [] });
    const waiting = await screen.findByTestId("mailto-waiting");
    expect(waiting.textContent).toContain("Add an account to write this message");
    expect(core.callsOf("create_draft")).toEqual([]);

    await user.click(within(waiting).getByRole("button", { name: "Add account" }));
    expect(useUiStore.getState().view).toEqual({ kind: "settings", section: "accounts" });
    expect(useUiStore.getState().accountsPage).toEqual({ kind: "add" });

    core.accounts.push(ADA);
    await emitCore({ type: "accountsChanged" });
    const composer = await composerForm();
    await waitFor(() => {
      expect(within(composer).getByLabelText<HTMLInputElement>("Subject").value).toBe("Later");
    });
    expect(core.callsOf("create_draft")[0]?.["request"]).toMatchObject({ kind: "new", accountId: ADA.id });
  });

  it("writes from the account picked in the switcher", async () => {
    const other = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "work@example.org" });
    useUiStore.setState({ selectedAccountId: other.id });
    const { core } = setup([makeComposeRequest({ to: [{ name: null, email: "grace@example.org" }] })], { accounts: [ADA, other] });
    await composerForm();
    expect(core.callsOf("create_draft")[0]?.["request"]).toMatchObject({ accountId: other.id });
  });

  it("a failure to read the launch links is shown and can be closed", async () => {
    const { user } = setup([], { failures: { take_launch_links: "framework said no" } });
    const error = await screen.findByTestId("mailto-launch-error");
    expect(error.textContent).toContain("The email links could not be read");
    expect(error.dataset["errorCode"]).toBe("internal");
    await user.click(within(error).getByRole("button", { name: "Close" }));
    expect(screen.queryByTestId("mailto-launch-error")).toBeNull();
  });

  it("a failed first save shows the error and leaves no half-filled draft behind", async () => {
    const { core } = setup([FULL], { failures: { save_draft: ipcError("database") } });
    const error = await screen.findByTestId("composer-open-error");
    expect(error.dataset["errorCode"]).toBe("database");
    const created = core.compose.drafts.size;
    expect(core.compose.deleted).toHaveLength(1);
    expect(created).toBe(0);
  });
});

describe("prefilling a draft", () => {
  it("puts the text into the HTML body of an HTML draft, escaped", async () => {
    const draft = makeDraft({ format: "html", accountId: ADA.id });
    const core = mockCore({ accounts: [ADA], compose: { drafts: [draft] } });
    const saved = await prefillDraft(draft, {
      cc: [],
      bcc: [],
      subject: null,
      body: "<b>not bold</b>\nline two",
      inReplyTo: null,
      notices: [],
    });
    const update = core.callsOf("save_draft")[0]?.["update"] as DraftUpdate;
    expect(update.bodyHtml).not.toContain("<b>");
    expect(update.bodyHtml).toContain("&lt;b&gt;not bold&lt;/b&gt;");
    expect(update.bodyText).toBe("");
    expect(update.subject).toBe("");
    expect(saved.bodyHtml).toBe(update.bodyHtml);
  });
});
