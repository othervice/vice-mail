// Stage 16 (decision 0036): attachment previews (image, PDF page by page, text), "Save all" into a
// folder (messages and `.eml` files), pins on attachments and the view "Pinned attachments", the
// palette entries, and the inbox's note on mail of blocked senders it leaves out.
import { act, screen, waitFor, within } from "@testing-library/react";
import { mockWindows } from "@tauri-apps/api/mocks";
import axe from "axe-core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { attachmentIpc } from "../src/services/ipc";
import type { AttachmentPreview, EmlMessageView, MessageListItem, PinnedAttachment, ThemePreference } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { htmlBody, ipcError, makeAccount, makeAttachment, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import type { AttachmentsFakeOptions } from "./support/attachments";
import { emitCore } from "./support/events";
import { closeOpenedMessage, renderApp } from "./support/render";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

const ADA = makeAccount({ writesEnabled: true });
const PNG = "data:image/png;base64,iVBORw0KGgo=";

const PHOTO = makeAttachment({ index: 0, name: "photo.png", mimeType: "image/png", size: 52_000, preview: "image" });
const REPORT = makeAttachment({ index: 1, name: "report.pdf", mimeType: "application/pdf", preview: "pdf" });
const NOTES = makeAttachment({ index: 2, name: "notes.txt", mimeType: "text/plain", size: 900, preview: "text" });
const TOOL = makeAttachment({ index: 3, name: "setup.exe", mimeType: "application/x-msdownload", executable: true, preview: null });

function grace(overrides: Partial<MessageListItem> = {}): MessageListItem {
  return makeMessage({
    subject: "Holiday photos",
    from: { name: "Grace Hopper", email: "grace@example.org" },
    hasAttachments: true,
    ...overrides,
  });
}

const image = (overrides: Partial<Extract<AttachmentPreview, { kind: "image" }>> = {}): AttachmentPreview => ({
  kind: "image",
  dataUrl: PNG,
  width: 640,
  height: 480,
  scaled: false,
  ...overrides,
});

const pdfPage = (page: number, pages = 3, morePages = false): AttachmentPreview => ({
  kind: "pdf",
  dataUrl: PNG,
  width: 600,
  height: 800,
  page,
  pages,
  morePages,
});

type Options = NonNullable<Parameters<typeof mockCore>[0]>;

/** The one message of the inbox (makeMessage gives every call a new id). */
const MESSAGE = grace();

function setup(attachments = [PHOTO, REPORT, NOTES, TOOL], attachmentOptions: AttachmentsFakeOptions = {}, extra: Options = {}) {
  const message = MESSAGE;
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: [message] },
    views: { [message.id]: makeView(message, { attachments }) },
    readingPreferences: { markReadOnOpen: "never" },
    attachments: {
      previews: {
        [`${String(message.id)}/0`]: image(),
        [`${String(message.id)}/1`]: (page) => pdfPage(page, 3, true),
        [`${String(message.id)}/2`]: { kind: "text", text: "<b>not bold</b>\nline two", truncated: true },
      },
      ...attachmentOptions,
    },
    ...extra,
  });
  return { core, message, ...renderApp() };
}

async function openMessage(user: ReturnType<typeof renderApp>["user"]) {
  const [option] = await screen.findAllByRole("option");
  if (!option) throw new Error("no row");
  await user.click(option);
  const pane = await screen.findByRole("region", { name: "Message" });
  return within(pane).getByTestId("attachments");
}

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

describe("the IPC contract of stage 16 attachments", () => {
  it("sends the command names and argument names of the contract", async () => {
    const core = mockCore({ attachments: { saveAll: { "0123456789abcdef0123456789abcdef": { kind: "cancelled" } } } });
    const fileId = "0123456789abcdef0123456789abcdef";
    await expect(attachmentIpc.previewAttachment(7, 1, 2)).rejects.toMatchObject({ code: "preview_unsupported" });
    await expect(attachmentIpc.previewEmlAttachment(fileId, 0, 0)).rejects.toMatchObject({ code: "preview_unsupported" });
    await attachmentIpc.saveAllAttachments(7);
    await expect(attachmentIpc.saveAllEmlAttachments(fileId)).resolves.toEqual({ kind: "cancelled" });
    await expect(attachmentIpc.getAttachmentPins(7)).resolves.toEqual([]);
    await expect(attachmentIpc.listPinnedAttachments({ kind: "all" })).resolves.toEqual([]);
    await expect(attachmentIpc.countBlockedHidden({ kind: "unified", view: { kind: "all" } })).resolves.toBe(0);
    await expect(attachmentIpc.setAttachmentPinned(7, 0, true)).rejects.toMatchObject({ code: "not_found" });
    expect(core.calls.map((call) => call.cmd)).toEqual([
      "preview_attachment",
      "preview_eml_attachment",
      "save_all_attachments",
      "save_all_eml_attachments",
      "get_attachment_pins",
      "list_pinned_attachments",
      "count_blocked_hidden",
      "set_attachment_pinned",
    ]);
    expect(core.calls.map((call) => call.args)).toEqual([
      { messageId: 7, index: 1, page: 2 },
      { fileId, index: 0, page: 0 },
      { messageId: 7 },
      { fileId },
      { messageId: 7 },
      { scope: { kind: "all" } },
      { scope: { kind: "unified", view: { kind: "all" } } },
      { messageId: 7, index: 0, pinned: true },
    ]);
  });
});

describe("attachment preview", () => {
  it("shows an image the core encoded, named by the attachment; Escape closes and the focus returns", async () => {
    const { core, message, user } = setup();
    const section = await openMessage(user);
    const button = within(section).getByRole("button", { name: "Preview photo.png" });
    await user.click(button);
    const dialog = await screen.findByRole("dialog", { name: "Preview of photo.png" });
    const img = await within(dialog).findByRole("img", { name: "photo.png" });
    expect(img.getAttribute("src")).toBe(PNG);
    expect(core.attachments.previews).toEqual([`${String(message.id)}/0/0`]);
    // Modal: the rest of the window is inert while it is open.
    expect(document.querySelector("#root, body > div:not(:last-child)")?.hasAttribute("inert")).toBe(true);

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(button);
  });

  it("opens from the card too: a click on it, or Space while it has the focus", async () => {
    const { user } = setup();
    const section = await openMessage(user);
    const [card] = within(section).getAllByTestId("attachment-card");
    if (!card) throw new Error("no card");
    await user.click(card);
    await screen.findByRole("dialog", { name: "Preview of photo.png" });
    await user.click(screen.getByRole("button", { name: "Close preview" }));
    expect(document.activeElement).toBe(card);

    act(() => {
      card.focus();
    });
    await user.keyboard(" ");
    await screen.findByRole("dialog", { name: "Preview of photo.png" });
  });

  it("offers no preview where the core has none", async () => {
    const { user } = setup();
    const section = await openMessage(user);
    const items = within(section).getAllByTestId("attachment");
    const tool = items.find((item) => item.textContent.includes("setup.exe"));
    if (!tool) throw new Error("no setup.exe");
    expect(within(tool).queryByRole("button", { name: /^Preview/ })).toBeNull();
    expect(within(tool).queryByTestId("attachment-card")).toBeNull();
  });

  it("pages through a PDF one page per call and says when it has more pages than it shows", async () => {
    const { core, message, user } = setup();
    const section = await openMessage(user);
    await user.click(within(section).getByRole("button", { name: "Preview report.pdf" }));
    const dialog = await screen.findByRole("dialog", { name: "Preview of report.pdf" });
    await within(dialog).findByRole("img", { name: "report.pdf, page 1" });
    expect(within(dialog).getByTestId("preview-page").textContent).toBe("Page 1 of 3");
    expect(within(dialog).getByTestId("preview-more-pages").textContent).toContain("more pages");
    expect(within(dialog).getByRole<HTMLButtonElement>("button", { name: "Previous page" }).disabled).toBe(true);

    await user.click(within(dialog).getByRole("button", { name: "Next page" }));
    await within(dialog).findByRole("img", { name: "report.pdf, page 2" });
    expect(within(dialog).getByTestId("preview-page").textContent).toBe("Page 2 of 3");
    await user.click(within(dialog).getByRole("button", { name: "Next page" }));
    await within(dialog).findByRole("img", { name: "report.pdf, page 3" });
    expect(within(dialog).getByRole<HTMLButtonElement>("button", { name: "Next page" }).disabled).toBe(true);
    await user.click(within(dialog).getByRole("button", { name: "Previous page" }));
    await within(dialog).findByRole("img", { name: "report.pdf, page 2" });
    const id = String(message.id);
    expect(core.attachments.previews).toEqual([`${id}/1/0`, `${id}/1/1`, `${id}/1/2`]);
  });

  it("shows text as text, never as markup, and says when it was cut", async () => {
    const { user } = setup();
    const section = await openMessage(user);
    await user.click(within(section).getByRole("button", { name: "Preview notes.txt" }));
    const dialog = await screen.findByRole("dialog", { name: "Preview of notes.txt" });
    const text = await within(dialog).findByTestId("preview-text");
    expect(text.tagName).toBe("PRE");
    expect(text.textContent).toBe("<b>not bold</b>\nline two");
    expect(text.querySelector("b")).toBeNull();
    expect(within(dialog).getByTestId("preview-truncated").textContent).toContain("Only the beginning");
  });

  it("shows the typed error with Save and Open; Open of a program still asks first", async () => {
    const tool = makeAttachment({ ...TOOL, preview: "image" });
    const { core, message, user } = setup([PHOTO, tool], {
      previews: { [`${String(MESSAGE.id)}/3`]: ipcError("preview_damaged", "bad header") },
    });
    const section = await openMessage(user);
    await user.click(within(section).getByRole("button", { name: "Preview setup.exe" }));
    const dialog = await screen.findByRole("dialog", { name: "Preview of setup.exe" });
    const error = await within(dialog).findByTestId("preview-error");
    expect(error.getAttribute("data-error-code")).toBe("preview_damaged");
    expect(error.textContent).toContain("could not be read for a preview");

    await user.click(within(dialog).getByRole("button", { name: "Open with another app" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    // The warning for files that can run code, as with the card's own Open.
    await screen.findByTestId("attachment-warning");
    expect(core.callsOf("open_attachment")).toEqual([]);

    await user.click(screen.getByTestId("attachment-cancel"));
    await user.click(within(section).getByRole("button", { name: "Preview setup.exe" }));
    await user.click(await screen.findByTestId("preview-save"));
    await waitFor(() => {
      expect(core.callsOf("save_attachment")).toEqual([{ messageId: message.id, index: 3 }]);
    });
  });

  it("refuses an image that is not one the core encodes (PNG or JPEG data URI)", async () => {
    const { user } = setup([PHOTO, REPORT], {
      previews: { [`${String(MESSAGE.id)}/0`]: image({ dataUrl: "data:image/svg+xml;base64,PHN2Zz4=" }) },
    });
    const section = await openMessage(user);
    await user.click(within(section).getByRole("button", { name: "Preview photo.png" }));
    const dialog = await screen.findByRole("dialog", { name: "Preview of photo.png" });
    expect((await within(dialog).findByTestId("preview-error")).getAttribute("data-error-code")).toBe("preview_damaged");
    expect(within(dialog).queryByRole("img")).toBeNull();
  });

  it("shows that it is loading while the core decodes", async () => {
    const { user } = setup(undefined, {}, { hang: ["preview_attachment"] });
    const section = await openMessage(user);
    await user.click(within(section).getByRole("button", { name: "Preview photo.png" }));
    const dialog = await screen.findByRole("dialog", { name: "Preview of photo.png" });
    expect(within(dialog).getByTestId("preview-loading").textContent).toContain("Loading the preview");
  });
});

describe("Save all", () => {
  it("saves every attachment into the chosen folder and says how many, renamed ones too", async () => {
    const message = MESSAGE;
    const { core, user } = setup(undefined, {
      saveAll: { [String(message.id)]: { kind: "saved", folder: "/Users/ada/Desktop/Trip", saved: 4, renamed: 1 } },
    });
    const section = await openMessage(user);
    await user.click(within(section).getByRole("button", { name: "Save all 4 attachments into a folder" }));
    const status = await within(section).findByTestId("attachments-saved-all");
    expect(status.textContent).toBe("4 files saved to /Users/ada/Desktop/Trip 1 renamed because the name was taken.");
    expect(core.attachments.savedAll).toEqual([String(message.id)]);
  });

  it("says nothing when the folder dialog was cancelled, and shows a typed error when it failed", async () => {
    const message = MESSAGE;
    const { user } = setup([PHOTO, REPORT], { saveAll: { [String(message.id)]: { kind: "cancelled" } } });
    const section = await openMessage(user);
    await user.click(within(section).getByRole("button", { name: "Save all 2 attachments into a folder" }));
    await waitFor(() => {
      expect(within(section).queryByTestId("attachments-saved-all")).toBeNull();
    });
    expect(within(section).queryByRole("alert")).toBeNull();
  });

  it("shows the core's error under the attachments", async () => {
    const { user } = setup([PHOTO, REPORT], {}, { failures: { save_all_attachments: ipcError("database") } });
    const section = await openMessage(user);
    await user.click(within(section).getByRole("button", { name: "Save all 2 attachments into a folder" }));
    const alert = await within(section).findByRole("alert");
    expect(alert.textContent).toContain("The attachments could not be saved.");
    expect(alert.getAttribute("data-error-code")).toBe("database");
  });

  it("is only offered from two attachments on", async () => {
    const { user } = setup([PHOTO]);
    const section = await openMessage(user);
    expect(within(section).queryByTestId("attachments-save-all")).toBeNull();
  });

  it("works for an opened .eml file, without pins", async () => {
    const fileId = "0123456789abcdef0123456789abcdef";
    const view: EmlMessageView = {
      fileId,
      fileName: "Trip.eml",
      header: { from: [{ name: "Grace", email: "grace@example.org" }], to: [], cc: [], bcc: [], replyTo: [], date: 1, subject: "Trip" },
      body: htmlBody("<p>Photos</p>"),
      formats: ["html"],
      attachments: [PHOTO, REPORT],
      problems: [],
    };
    window.history.replaceState(null, "", `/?window=eml&fileId=${fileId}`);
    mockWindows(`message-file-${fileId}`);
    const core = mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      eml: { files: { [fileId]: view } },
      attachments: {
        saveAll: { [fileId]: { kind: "saved", folder: "/Users/ada/Downloads", saved: 2, renamed: 0 } },
        previews: { [`${fileId}/0`]: image() },
      },
    });
    const { user } = renderApp();
    const section = await screen.findByTestId("attachments");
    expect(within(section).queryByRole("button", { name: /^Pin / })).toBeNull();
    await user.click(within(section).getByRole("button", { name: "Save all 2 attachments into a folder" }));
    expect((await within(section).findByTestId("attachments-saved-all")).textContent).toBe("2 files saved to /Users/ada/Downloads");
    expect(core.callsOf("save_all_eml_attachments")).toEqual([{ fileId }]);

    await user.click(within(section).getByRole("button", { name: "Preview photo.png" }));
    await screen.findByRole("img", { name: "photo.png" });
    expect(core.callsOf("preview_eml_attachment")).toEqual([{ fileId, index: 0, page: 0 }]);
  });
});

describe("pinned attachments", () => {
  it("pins an attachment of the message, shows it pinned and unpins it", async () => {
    const { core, message, user } = setup([PHOTO, REPORT]);
    const section = await openMessage(user);
    await user.click(await within(section).findByRole("button", { name: "Pin report.pdf" }));
    await within(section).findByRole("button", { name: "Unpin report.pdf" });
    expect(within(section).getByTestId("attachment-pinned").textContent).toBe("Pinned");
    expect(core.callsOf("set_attachment_pinned")).toEqual([{ messageId: message.id, index: 1, pinned: true }]);
    expect(core.attachments.pinned.map((entry) => entry.name)).toEqual(["report.pdf"]);

    await user.click(within(section).getByRole("button", { name: "Unpin report.pdf" }));
    await within(section).findByRole("button", { name: "Pin report.pdf" });
    expect(core.attachments.pinned).toEqual([]);
  });

  it("follows a pin made on another device (inboxChanged)", async () => {
    const message = MESSAGE;
    const pinned: PinnedAttachment = { message, index: 0, name: "photo.png", mimeType: "image/png", size: 52_000, preview: "image", pinnedAt: 1 };
    const { core, user } = setup([PHOTO, REPORT]);
    const section = await openMessage(user);
    await within(section).findByRole("button", { name: "Pin photo.png" });
    // The settings sync took a pin over: the core holds it now and announces it.
    (core.attachments.pinned as PinnedAttachment[]).push(pinned);
    await emitCore({ type: "inboxChanged", accountId: ADA.id });
    await within(section).findByRole("button", { name: "Unpin photo.png" });
  });

  it("shows a typed error when the pin cannot be changed", async () => {
    const { user } = setup([PHOTO, REPORT], {}, { failures: { set_attachment_pinned: ipcError("database") } });
    const section = await openMessage(user);
    await user.click(await within(section).findByRole("button", { name: "Pin photo.png" }));
    const alert = await within(section).findByRole("alert");
    expect(alert.textContent).toContain("The pin could not be changed.");
  });

  function pinnedEntry(message: MessageListItem, overrides: Partial<PinnedAttachment> = {}): PinnedAttachment {
    return { message, index: 1, name: "report.pdf", mimeType: "application/pdf", size: 1_258_291, preview: "pdf", pinnedAt: 2, ...overrides };
  }

  function showPinned() {
    act(() => {
      useUiStore.getState().showMailView("pinnedAttachments");
    });
  }

  it("the view lists them with type, size and their message, and leads to the message", async () => {
    const message = MESSAGE;
    const { core, user } = setup([PHOTO, REPORT], { pinned: [pinnedEntry(message)] });
    showPinned();
    const list = await screen.findByRole("list", { name: "Pinned attachments" });
    const [row] = within(list).getAllByTestId("pinned-attachment");
    if (!row) throw new Error("no row");
    expect(row.textContent).toContain("report.pdf");
    expect(row.textContent).toContain("PDF, 1.2 MB");
    expect(row.textContent).toContain("Holiday photos");
    expect(row.textContent).toContain("From Grace Hopper");
    expect(core.callsOf("list_pinned_attachments")).toEqual([{ scope: { kind: "account", accountId: ADA.id } }]);

    await user.click(within(row).getByRole("button", { name: "Open the message of report.pdf" }));
    const pane = await screen.findByRole("region", { name: "Message" });
    expect(pane.textContent).toContain("Holiday photos");
    expect(useUiStore.getState().view).toEqual({ kind: "mailbox", accountId: ADA.id, mailboxId: message.mailboxId });
  });

  it("previews, saves and unpins from the view", async () => {
    const message = MESSAGE;
    const { core, user } = setup([PHOTO, REPORT], { pinned: [pinnedEntry(message)] });
    showPinned();
    const list = await screen.findByRole("list", { name: "Pinned attachments" });
    await user.click(within(list).getByRole("button", { name: "Preview report.pdf" }));
    const dialog = await screen.findByRole("dialog", { name: "Preview of report.pdf" });
    await within(dialog).findByRole("img", { name: "report.pdf, page 1" });
    await user.keyboard("{Escape}");

    await user.click(within(list).getByRole("button", { name: "Save report.pdf" }));
    await screen.findByText("report.pdf saved to /Users/ada/Downloads/report.pdf");
    expect(core.callsOf("save_attachment")).toEqual([{ messageId: message.id, index: 1 }]);

    await user.click(within(list).getByRole("button", { name: "Unpin report.pdf" }));
    await screen.findByTestId("pinned-attachments-empty");
    expect(screen.getByText("No pinned attachments")).toBeTruthy();
  });

  it("says so honestly when there are none, and shows a typed error when they cannot be loaded", async () => {
    setup([PHOTO]);
    showPinned();
    expect((await screen.findByTestId("pinned-attachments-empty")).textContent).toContain("never written to the server");
  });

  it("shows the load error with a retry", async () => {
    setup([PHOTO], {}, { failures: { list_pinned_attachments: ipcError("database") } });
    showPinned();
    const error = await screen.findByTestId("pinned-attachments-error");
    expect(error.textContent).toContain("could not be loaded");
    expect(within(error).getByRole("button", { name: "Try again" })).toBeTruthy();
  });
});

describe("palette", () => {
  async function openPalette(user: ReturnType<typeof renderApp>["user"]) {
    await user.keyboard("{Meta>}k{/Meta}");
    return screen.findByRole("dialog", { name: "Command palette" });
  }

  const entry = (palette: HTMLElement, label: string) =>
    within(palette)
      .getAllByRole("option")
      .find((option) => option.textContent.includes(label));

  it("goes to the pinned attachments and saves all attachments of the open message", async () => {
    const message = MESSAGE;
    const { core, user } = setup([PHOTO, REPORT], {
      saveAll: { [String(message.id)]: { kind: "saved", folder: "/Users/ada/Downloads", saved: 2, renamed: 0 } },
    });
    await screen.findAllByRole("option");
    // Stage 18 (task F): the folder opened with its top message; without one the entry waits.
    await closeOpenedMessage(user);
    let palette = await openPalette(user);
    expect(entry(palette, "Save all attachments")?.getAttribute("aria-disabled")).toBe("true");
    expect(entry(palette, "Go to Pinned attachments")).toBeTruthy();
    await user.keyboard("{Escape}");

    const section = await openMessage(user);
    palette = await openPalette(user);
    const saveAll = entry(palette, "Save all attachments");
    if (!saveAll) throw new Error("no entry");
    expect(saveAll.getAttribute("aria-disabled")).not.toBe("true");
    await user.click(saveAll);
    expect((await within(section).findByTestId("attachments-saved-all")).textContent).toBe("2 files saved to /Users/ada/Downloads");
    expect(core.callsOf("save_all_attachments")).toEqual([{ messageId: message.id }]);

    palette = await openPalette(user);
    const go = entry(palette, "Go to Pinned attachments");
    if (!go) throw new Error("no entry");
    await user.click(go);
    expect(useUiStore.getState().view).toEqual({ kind: "mail", mailView: "pinnedAttachments" });
  });
});

describe("hidden mail of blocked senders", () => {
  it("the inbox says how many it leaves out and leads to the blocked senders", async () => {
    const { core, user } = setup([PHOTO], { blockedHidden: { [JSON.stringify({ kind: "mailbox", mailboxId: 1 })]: 3 } });
    const note = await screen.findByTestId("blocked-hidden-note");
    expect(note.textContent).toContain("3 messages from blocked senders hidden");
    expect(core.callsOf("count_blocked_hidden")).toEqual([{ scope: { kind: "mailbox", mailboxId: 1 } }]);

    await user.click(within(note).getByRole("button", { name: "Show blocked senders in Settings" }));
    expect(useUiStore.getState().view).toEqual({ kind: "settings", section: "inbox" });
    const heading = await screen.findByRole("heading", { name: "Blocked senders" });
    await waitFor(() => {
      expect(document.activeElement).toBe(heading);
    });
  });

  it("uses the singular for one, says nothing for none, and asks again on inboxChanged", async () => {
    const { core } = setup([PHOTO]);
    await screen.findAllByRole("option");
    await waitFor(() => {
      expect(core.callsOf("count_blocked_hidden")).toHaveLength(1);
    });
    expect(screen.queryByTestId("blocked-hidden-note")).toBeNull();
    core.attachments.setBlockedHidden({ kind: "mailbox", mailboxId: 1 }, 1);
    await emitCore({ type: "inboxChanged", accountId: ADA.id });
    expect((await screen.findByTestId("blocked-hidden-note")).textContent).toContain("1 message from blocked senders hidden");
  });

  it("asks again when a sender is unblocked in Settings", async () => {
    const scope = { kind: "mailbox" as const, mailboxId: 1 };
    const { core, user } = setup([PHOTO], { blockedHidden: { [JSON.stringify(scope)]: 2 } }, {
      parity: { blockedSenders: [{ address: "promo@shop.example", createdAt: 1 }] },
    });
    await screen.findByTestId("blocked-hidden-note");
    const before = core.callsOf("count_blocked_hidden").length;
    act(() => {
      useUiStore.getState().showSettings("inbox");
    });
    core.attachments.setBlockedHidden(scope, 0);
    await user.click(await screen.findByRole("button", { name: "Unblock promo@shop.example" }));
    act(() => {
      useUiStore.getState().showMailView("inbox");
    });
    await waitFor(() => {
      expect(core.callsOf("count_blocked_hidden").length).toBeGreaterThan(before);
    });
    await waitFor(() => {
      expect(screen.queryByTestId("blocked-hidden-note")).toBeNull();
    });
  });

  it("the unified inbox has the note too, for its own scope", async () => {
    const bob = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "bob@vicemail.test", displayName: "Bob", color: 2 });
    const unified = JSON.stringify({ kind: "unified", view: { kind: "all" } });
    mockCore({ accounts: [ADA, bob], mailboxes: standardMailboxes(), messages: { 1: [grace()] }, attachments: { blockedHidden: { [unified]: 5 } } });
    renderApp();
    act(() => {
      useUiStore.getState().showUnifiedInbox({ kind: "all" });
    });
    const inbox = await screen.findByTestId("unified-inbox");
    expect((await within(inbox).findByTestId("blocked-hidden-note")).textContent).toContain("5 messages from blocked senders hidden");
  });
});

describe.each(["light", "dark"] as const)("accessibility in %s mode", (theme: ThemePreference) => {
  it("the attachments with pins and Save all, and the preview dialog, have no serious axe findings", async () => {
    const message = MESSAGE;
    const { user } = setup(undefined, {}, { preferences: { theme, language: "system" } });
    void message;
    const section = await openMessage(user);
    await within(section).findByRole("button", { name: "Pin photo.png" });
    expect(await seriousFindings()).toEqual([]);
    for (const name of ["photo.png", "report.pdf", "notes.txt"]) {
      await user.click(within(section).getByRole("button", { name: `Preview ${name}` }));
      const dialog = await screen.findByRole("dialog", { name: `Preview of ${name}` });
      await waitFor(() => {
        expect(within(dialog).queryByTestId("preview-loading")).toBeNull();
      });
      expect(await seriousFindings()).toEqual([]);
      await user.keyboard("{Escape}");
    }
  });

  it("the pinned attachments view and the blocked note have no serious axe findings", async () => {
    const message = MESSAGE;
    setup(
      [PHOTO, REPORT],
      {
        pinned: [{ message, index: 0, name: "photo.png", mimeType: "image/png", size: 52_000, preview: "image", pinnedAt: 1 }],
        blockedHidden: { [JSON.stringify({ kind: "mailbox", mailboxId: 1 })]: 2 },
      },
      { preferences: { theme, language: "system" } },
    );
    await screen.findByTestId("blocked-hidden-note");
    expect(await seriousFindings()).toEqual([]);
    act(() => {
      useUiStore.getState().showMailView("pinnedAttachments");
    });
    await screen.findByRole("list", { name: "Pinned attachments" });
    expect(await seriousFindings()).toEqual([]);
  });
});
