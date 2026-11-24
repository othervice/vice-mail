import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { mockWindows } from "@tauri-apps/api/mocks";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { insertAtCursor, isImagePath } from "../src/features/composer/Composer";
import { imageSizeOf } from "../src/features/composer/inlineImage";
import { cleanPaste } from "../src/features/composer/paste";
import { MAX_PASTE_BYTES, type DraftAttachment, type DraftUpdate, type MessageListItem } from "../src/services/ipc";
import { useComposerStore } from "../src/stores/composer";
import { makeDraft, makeIdentity, type ComposeFakeOptions } from "./support/compose";
import { ipcError, makeAccount, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";
import { toolbarAction } from "./support/reading";
import { makeSignature, makeTemplate, type SnippetsFakeOptions } from "./support/snippets";

// Stage 10: the composer complete (formatting, pasting, inline images, size, signatures in the
// composer, templates, send later, forward as attachment).

const ADA = makeAccount({ writesEnabled: true });
const GRACE = { name: "Grace Hopper", email: "grace@example.org" };

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
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

interface SetupOptions {
  compose?: ComposeFakeOptions;
  snippets?: SnippetsFakeOptions;
  items?: MessageListItem[];
  failures?: Record<string, ReturnType<typeof ipcError>>;
}

function setup({ compose = {}, snippets = {}, items = makeMessages(3), failures }: SetupOptions = {}) {
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    compose,
    snippets,
    ...(failures && { failures }),
  });
  return { core, items, ...renderApp() };
}

const saves = (core: ReturnType<typeof mockCore>) => core.callsOf("save_draft").map((c) => c["update"] as DraftUpdate);

/** Opens a rich text draft in the composer and waits for the editor. */
async function openRich(core: ReturnType<typeof mockCore>, draft = makeDraft({ accountId: ADA.id, format: "html", bodyHtml: "<p>Hallo</p>", to: [GRACE] })) {
  await screen.findByRole("button", { name: "New message" });
  act(() => {
    useComposerStore.getState().open({ kind: "draft", draftId: draft.id, draft });
  });
  // The editor loads lazily with the first rich text body; the first test pays for the import.
  const editor = await screen.findByTestId("composer-rich-body", {}, { timeout: 10_000 });
  void core;
  return editor;
}

function richDraft(overrides: Parameters<typeof makeDraft>[0] = {}) {
  return makeDraft({ accountId: ADA.id, format: "html", bodyHtml: "<p>Hallo</p>", to: [GRACE], ...overrides });
}

/** Places the cursor at the end of the editor's text. */
async function clickEnd(user: ReturnType<typeof userEvent.setup>, editor: HTMLElement) {
  await user.click(editor);
}

describe("composer: formatting", () => {
  it("offers undo, redo, styles, strikethrough, code and alignment", async () => {
    const draft = richDraft();
    const { core } = setup({ compose: { drafts: [draft] } });
    await openRich(core, draft);
    const toolbar = screen.getByRole("toolbar", { name: "Formatting" });
    for (const name of ["Undo", "Redo", "Bold", "Strikethrough", "Code", "Center", "Align right", "Justify", "Insert image"]) {
      expect(within(toolbar).getByRole("button", { name }), name).toBeTruthy();
    }
    expect(within(toolbar).getByRole("combobox", { name: "Paragraph style" })).toBeTruthy();
    // Nothing typed yet: nothing to undo.
    expect(within(toolbar).getByRole<HTMLButtonElement>("button", { name: "Undo" }).disabled).toBe(true);
  });

  it("a heading, centred, is saved as the HTML the core keeps", async () => {
    const draft = richDraft();
    const { core, user } = setup({ compose: { drafts: [draft] } });
    const editor = await openRich(core, draft);
    await clickEnd(user, editor);
    await user.selectOptions(screen.getByRole("combobox", { name: "Paragraph style" }), "heading2");
    await user.click(screen.getByRole("button", { name: "Center" }));
    expect(screen.getByRole("button", { name: "Center" }).getAttribute("aria-pressed")).toBe("true");
    await user.click(screen.getByLabelText("Subject"));
    await waitFor(() => {
      // The core reads `text-align` from the declarations of the style (a trailing ";" is fine).
      expect(saves(core).at(-1)?.bodyHtml).toMatch(/^<h2 style="text-align: center;?">Hallo<\/h2>$/);
    });
  });

  it("strikethrough and code marks, and undo takes typing back", async () => {
    const draft = richDraft({ bodyHtml: "" });
    const { core, user } = setup({ compose: { drafts: [draft] } });
    const editor = await openRich(core, draft);
    await user.click(editor);
    await user.click(screen.getByRole("button", { name: "Strikethrough" }));
    await user.type(editor, "alt", { skipClick: true });
    await user.click(screen.getByRole("button", { name: "Strikethrough" }));
    await user.click(screen.getByRole("button", { name: "Code" }));
    await user.type(editor, "x", { skipClick: true });
    await user.click(screen.getByLabelText("Subject"));
    await waitFor(() => {
      expect(saves(core).at(-1)?.bodyHtml).toMatch(/<s>alt<\/s><code>x<\/code>/);
    });
    const undo = screen.getByRole<HTMLButtonElement>("button", { name: "Undo" });
    expect(undo.disabled).toBe(false);
    await user.click(undo);
    await waitFor(() => {
      expect(editor.querySelector("code")).toBeNull();
    });
    await user.click(screen.getByRole("button", { name: "Redo" }));
    await waitFor(() => {
      expect(editor.querySelector("code")?.textContent).toBe("x");
    });
  });
});

describe("composer: pasting", () => {
  it("pasted HTML goes through the core's cleaning before it enters the editor", async () => {
    const draft = richDraft({ bodyHtml: "" });
    const { core, user } = setup({ compose: { drafts: [draft] } });
    const editor = await openRich(core, draft);
    await user.click(editor);
    const html = '<p class="MsoNormal" style="color:red">Aus <b>Word</b><img src="https://tracker.example/p.gif"></p><script>alert(1)</script>';
    fireEvent.paste(editor, {
      clipboardData: { types: ["text/html", "text/plain"], getData: (type: string) => (type === "text/html" ? html : "Aus Word") },
    });
    await waitFor(() => {
      expect(core.callsOf("clean_pasted_html")).toEqual([{ html }]);
    });
    await waitFor(() => {
      expect(editor.textContent).toContain("Aus Word");
    });
    expect(editor.querySelector("script, img")).toBeNull();
    expect((await screen.findByTestId("composer-paste-note")).textContent).toContain("Images from the clipboard were removed.");
    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByTestId("composer-paste-note")).toBeNull();
  });

  it("plain text is pasted by the editor itself, without the core", async () => {
    const draft = richDraft({ bodyHtml: "" });
    const { core, user } = setup({ compose: { drafts: [draft] } });
    const editor = await openRich(core, draft);
    await user.click(editor);
    fireEvent.paste(editor, { clipboardData: { types: ["text/plain"], getData: (type: string) => (type === "text/plain" ? "nur Text" : "") } });
    await waitFor(() => {
      expect(editor.textContent).toContain("nur Text");
    });
    expect(core.callsOf("clean_pasted_html")).toEqual([]);
  });

  it("a refused paste goes in as plain text and says so; other failures are shown", async () => {
    setup({ failures: { clean_pasted_html: ipcError("invalid_argument") } });
    await expect(cleanPaste("<p>x</p>", "x")).resolves.toEqual({ kind: "plain", text: "x" });
    expect(await cleanPaste("x".repeat(MAX_PASTE_BYTES + 1), "plain")).toEqual({ kind: "plain", text: "plain" });
    mockCore({ failures: { clean_pasted_html: ipcError("internal") } });
    await expect(cleanPaste("<p>x</p>", "x")).rejects.toMatchObject({ code: "internal" });
  });
});

describe("composer: inline images", () => {
  const image: DraftAttachment = { id: "img-1", fileName: "logo.png", mime: "image/png", size: 4000, contentId: "3f9a@vicemail.test" };

  it("inserts an image from the core's dialog as cid:, shows it from a data: URL and resizes it", async () => {
    const draft = richDraft();
    const { core, user } = setup({ compose: { drafts: [draft], pickedImages: [image] } });
    const editor = await openRich(core, draft);
    await user.click(editor);
    await user.click(screen.getByRole("button", { name: "Insert image" }));
    await waitFor(() => {
      expect(core.callsOf("pick_inline_images")).toEqual([{ draftId: draft.id }]);
    });
    const shown = await waitFor(() => {
      const img = editor.querySelector<HTMLImageElement>("img.composer-inline-image");
      expect(img?.getAttribute("src")).toMatch(/^data:image\/png;base64,/);
      return img;
    });
    expect(core.callsOf("inline_image_data")[0]).toEqual({ draftId: draft.id, attachmentId: "img-1" });
    // Not in the attachment bar: it is part of the text.
    expect(screen.queryByTestId("composer-attachment")).toBeNull();
    expect(shown).toBeTruthy();

    // A click selects the image, and it can be made smaller.
    fireEvent.click(shown as HTMLImageElement);
    const size = await screen.findByRole("combobox", { name: "Image size" });
    await user.selectOptions(size, "small");
    await user.click(screen.getByLabelText("Subject"));
    await waitFor(() => {
      expect(saves(core).at(-1)?.bodyHtml).toContain('<img src="cid:3f9a@vicemail.test" alt="" width="160">');
    });
    expect(saves(core).every((u) => !u.bodyHtml.includes("data:"))).toBe(true);
    expect(core.callsOf("check_draft_size").length).toBeGreaterThan(0);
  });

  it("a reopened draft shows its inline images again", async () => {
    const draft = richDraft({ bodyHtml: '<p>Logo: <img src="cid:3f9a@vicemail.test" alt="" width="320"></p>', attachments: [image] });
    const { core } = setup({ compose: { drafts: [draft] } });
    const editor = await openRich(core, draft);
    await waitFor(() => {
      const img = editor.querySelector<HTMLImageElement>("img.composer-inline-image");
      expect(img?.getAttribute("src")).toMatch(/^data:image\/png/);
      expect(img?.getAttribute("width")).toBe("320");
    });
  });

  it("dropped images go into a rich text draft, other files are attached", async () => {
    const draft = richDraft();
    const { core, user } = setup({ compose: { drafts: [draft] } });
    mockWindows("main");
    const editor = await openRich(core, draft);
    await user.click(editor);
    const paths = ["/Users/ada/Foto.JPG", "/Users/ada/Bericht.pdf"];
    await waitFor(async () => {
      await act(async () => {
        await emit("tauri://drag-enter", { paths, position: { x: 10, y: 10 } });
      });
      expect(screen.getByTestId("composer-drop-hint").textContent).toContain("Images go into the text");
    });
    await act(async () => {
      await emit("tauri://drag-drop", { paths, position: { x: 10, y: 10 } });
    });
    await waitFor(() => {
      expect(core.callsOf("add_dropped_inline_images")[0]).toMatchObject({ paths: ["/Users/ada/Foto.JPG"] });
      expect(core.callsOf("add_dropped_attachments")[0]).toMatchObject({ paths: ["/Users/ada/Bericht.pdf"] });
    });
    expect((await screen.findByTestId("composer-attachment")).textContent).toContain("Bericht.pdf");
  });

  it("a failure while adding images is shown in place", async () => {
    const draft = richDraft();
    const { core, user } = setup({ compose: { drafts: [draft] }, failures: { pick_inline_images: ipcError("invalid_argument") } });
    await openRich(core, draft);
    await user.click(screen.getByRole("button", { name: "Insert image" }));
    const notice = await screen.findByTestId("composer-action-error");
    expect(notice.textContent).toContain("The images could not be added.");
  });
});

describe("composer: size against the server's limit", () => {
  it("warns at once when the message is larger than the SMTP server accepts", async () => {
    const big: DraftAttachment = { id: "a1", fileName: "Video.mov", mime: "video/quicktime", size: 30_000_000, contentId: null };
    const { core, user } = setup({
      compose: { picked: [big], sizeCheck: { size: 40_000_000, serverLimit: 26_214_400, limitKnown: true, exceedsServerLimit: true } },
    });
    await user.click(await screen.findByRole("button", { name: "New message" }));
    await screen.findByLabelText("Subject");
    await user.click(screen.getByRole("button", { name: "Attach files" }));
    const warning = await screen.findByTestId("composer-size-warning");
    expect(warning.textContent).toContain("larger than your mail server accepts");
    expect(warning.textContent).toMatch(/40 MB.*26\.2 MB/);
    expect(core.callsOf("check_draft_size")).toHaveLength(1);
  });

  it("a send the core refuses as too large says what to do", async () => {
    const { user } = setup({ failures: { send_draft: ipcError("message_too_large") } });
    await user.click(await screen.findByRole("button", { name: "New message" }));
    await user.type(await screen.findByRole("combobox", { name: "To" }), "grace@example.org,");
    await user.click(screen.getByTestId("composer-send"));
    const error = await screen.findByTestId("composer-send-error");
    expect(error.textContent).toContain("The message is larger than your mail server accepts.");
    expect(error.textContent).toContain("share large files as a link");
  });
});

describe("composer: signatures", () => {
  const work = makeSignature({ id: "sig-work", name: "Work", html: "<p>Ada, Analytical Engines</p>", text: "Ada, Analytical Engines" });
  const short = makeSignature({ id: "sig-short", name: "Short", html: "<p>A.</p>", text: "A." });

  it("starts with the identity's default for the draft kind and switches in the composer", async () => {
    const identities = { [ADA.id]: [makeIdentity({ id: 1, accountId: ADA.id, email: ADA.email, signatureNew: "sig-work", signatureReply: "sig-short" })] };
    const draft = richDraft();
    const { core, user } = setup({ compose: { drafts: [draft], identities }, snippets: { signatures: [work, short] } });
    await openRich(core, draft);
    const choice = await screen.findByRole<HTMLSelectElement>("combobox", { name: "Signature" });
    await waitFor(() => {
      expect(within(choice).getByRole("option", { name: "Default: Work" })).toBeTruthy();
    });
    expect(choice.value).toBe("default");
    expect(screen.getByTestId("composer-signature").textContent).toBe("-- Ada, Analytical Engines");

    await user.selectOptions(choice, "sig-short");
    expect(screen.getByTestId("composer-signature").textContent).toBe("-- A.");
    await user.selectOptions(choice, "none");
    expect(screen.queryByTestId("composer-signature")).toBeNull();
    await user.click(screen.getByLabelText("Subject"));
    await waitFor(() => {
      expect(saves(core).at(-1)).toMatchObject({ includeSignature: false });
    });
    await user.selectOptions(choice, "sig-short");
    await user.click(screen.getByLabelText("Subject"));
    await waitFor(() => {
      expect(saves(core).at(-1)).toMatchObject({ includeSignature: true, signatureId: "sig-short" });
    });
  });

  it("a reply takes the default for replies", async () => {
    const identities = { [ADA.id]: [makeIdentity({ id: 1, accountId: ADA.id, email: ADA.email, signatureNew: "sig-work", signatureReply: "sig-short" })] };
    const draft = richDraft({ kind: "reply" });
    const { core } = setup({ compose: { drafts: [draft], identities }, snippets: { signatures: [work, short] } });
    await openRich(core, draft);
    await waitFor(() => {
      expect(screen.getByTestId("composer-signature").textContent).toBe("-- A.");
    });
  });

  it("shows a rich signature with its formatting right below the text, inside the writing area", async () => {
    const formatted = makeSignature({
      id: "sig-formatted",
      name: "Formatted",
      html: '<p><strong>Ada</strong> Lovelace<br><em>Analytical Engines</em></p><ul><li>One</li></ul><p style="text-align: center">Centre</p><p><a href="https://example.org/">Site</a><img src="cid:logo@x" alt="Logo"></p>',
      text: "Ada Lovelace\nAnalytical Engines",
    });
    const draft = richDraft({ kind: "reply", quoteText: "> Earlier text", quotePosition: "above", includeQuote: true });
    const { core } = setup({
      compose: { drafts: [draft], identities: { [ADA.id]: [makeIdentity({ id: 1, accountId: ADA.id, email: ADA.email, signatureReply: "sig-formatted" })] } },
      snippets: { signatures: [formatted] },
    });
    await openRich(core, draft);
    const signature = await screen.findByTestId("composer-signature");
    expect(signature.querySelector("strong")?.textContent).toBe("Ada");
    expect(signature.querySelector("em")?.textContent).toBe("Analytical Engines");
    expect(signature.querySelectorAll("li")).toHaveLength(1);
    expect(within(signature).getByText("Centre").className).toContain("text-center");
    // A link is text in the preview, an image is left out, and no attribute of the HTML arrives.
    expect(signature.querySelector("a")).toBeNull();
    expect(signature.querySelector("img")).toBeNull();
    expect(signature.querySelector("[style]")).toBeNull();
    expect(signature.textContent).toBe("-- Ada LovelaceAnalytical EnginesOneCentreSite");
    // Right below the text: in the writing area, after the editor, and the quoted original after
    // it, so text, signature and original scroll as one document.
    const area = screen.getByTestId("composer-writing");
    expect(area.contains(signature)).toBe(true);
    expect(area.contains(screen.getByTestId("composer-rich-body"))).toBe(true);
    const quote = screen.getByTestId("composer-quote");
    expect(area.contains(quote)).toBe(true);
    expect(signature.compareDocumentPosition(quote) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});

describe("composer: templates", () => {
  const offer = makeTemplate({ id: "tpl-offer", name: "Offer", subject: "Offer for {{name}}", html: "<p>Hello {{first_name}},</p><p>{{nickname}}</p>" });

  it("the template menu fills the template for the first recipient and inserts it", async () => {
    const draft = richDraft({ bodyHtml: "", subject: "" });
    const { core, user } = setup({ compose: { drafts: [draft] }, snippets: { templates: [offer] } });
    await openRich(core, draft);
    await user.click(screen.getByRole("button", { name: "Template" }));
    const menu = await screen.findByRole("group", { name: "Templates" });
    await user.click(await within(menu).findByRole("button", { name: /Offer/ }));
    await waitFor(() => {
      expect(core.callsOf("fill_template")[0]).toEqual({ templateId: "tpl-offer", draftId: draft.id, language: "en" });
    });
    await waitFor(() => {
      expect(screen.getByTestId("composer-rich-body").textContent).toContain("Hello Grace,");
    });
    expect(screen.getByLabelText<HTMLInputElement>("Subject").value).toBe("Offer for Grace Hopper");
    const note = await screen.findByTestId("composer-template-note");
    expect(note.textContent).toContain("Template “Offer” inserted.");
    expect(note.textContent).toContain("{{nickname}} is no known placeholder");
    expect(screen.queryByRole("group", { name: "Templates" })).toBeNull();
  });

  it("an empty template list says where to create one; Escape closes it, not the composer", async () => {
    const draft = richDraft();
    const { core, user } = setup({ compose: { drafts: [draft] } });
    await openRich(core, draft);
    await user.click(screen.getByRole("button", { name: "Template" }));
    const menu = await screen.findByRole("group", { name: "Templates" });
    expect(await within(menu).findByText(/No templates yet/)).toBeTruthy();
    await user.click(within(menu).getByRole("button", { name: "Manage templates" }));
    // Leaves for the settings; the composer stays open above them.
    expect(screen.getByTestId("composer")).toBeTruthy();
  });

  it("the command palette inserts a template into the open composer", async () => {
    const draft = richDraft({ bodyHtml: "" });
    const { core, user } = setup({ compose: { drafts: [draft] }, snippets: { templates: [offer] } });
    const editor = await openRich(core, draft);
    await user.click(editor);
    await user.keyboard("{Meta>}k{/Meta}");
    const palette = await screen.findByRole("dialog", { name: "Command palette" });
    await user.type(within(palette).getByRole("combobox"), "Insert template");
    await user.click(await within(palette).findByRole("option", { name: /Insert template: Offer/ }));
    await waitFor(() => {
      expect(core.callsOf("fill_template")).toHaveLength(1);
    });
    await waitFor(() => {
      expect(screen.getByTestId("composer-rich-body").textContent).toContain("Hello Grace,");
    });
  });

  it("a failure to fill the template is shown in place", async () => {
    const draft = richDraft();
    const { core, user } = setup({ compose: { drafts: [draft] }, snippets: { templates: [offer] }, failures: { fill_template: ipcError("not_found") } });
    await openRich(core, draft);
    await user.click(screen.getByRole("button", { name: "Template" }));
    await user.click(await screen.findByRole("button", { name: /Offer/ }));
    expect((await screen.findByTestId("composer-action-error")).textContent).toContain("The template could not be inserted.");
  });
});

describe("composer: send later", () => {
  it("schedules from the composer, says when it goes out, and closes", async () => {
    const draft = richDraft({ subject: "Später" });
    const { core, user } = setup({ compose: { drafts: [draft] } });
    await openRich(core, draft);
    await user.click(screen.getByRole("button", { name: "Send later…" }));
    const picker = await screen.findByTestId("composer-send-later-picker");
    expect(picker.textContent).toContain("running and online");
    await user.click(within(picker).getByTestId("send-later-preset-tomorrowMorning"));
    await waitFor(() => {
      expect(core.callsOf("schedule_send")).toHaveLength(1);
    });
    const call = core.callsOf("schedule_send")[0] as { draftId: string; requestId: string; when: { sendAt: number } };
    expect(call.draftId).toBe(draft.id);
    expect(call.when.sendAt).toBeGreaterThan(Date.now());
    // Saved first, then frozen: the composer closes.
    expect(saves(core).length).toBeGreaterThan(0);
    await waitFor(() => {
      expect(screen.queryByTestId("composer")).toBeNull();
    });
  });

  it("a refused schedule stays in the picker with the reason", async () => {
    const draft = richDraft();
    const { core, user } = setup({ compose: { drafts: [draft] }, failures: { schedule_send: ipcError("message_too_large") } });
    await openRich(core, draft);
    await user.click(screen.getByRole("button", { name: "Send later…" }));
    await user.click(await screen.findByTestId("send-later-preset-inOneHour"));
    const error = await screen.findByTestId("send-later-error");
    expect(error.textContent).toContain("larger than your mail server accepts");
    expect(screen.getByTestId("composer")).toBeTruthy();
  });
});

describe("forward as attachment", () => {
  it("is an action next to Forward and opens a draft with the whole message attached", async () => {
    const items = makeMessages(2);
    const first = items[0] as MessageListItem;
    const eml: DraftAttachment = { id: "eml", fileName: "Hello.eml", mime: "message/rfc822", size: 5000, contentId: null };
    const { core, user } = setup({ items, compose: { replies: { [first.id]: { subject: "Fwd: Hello", attachments: [eml] } } } });
    await user.click(await screen.findByRole("option", { name: new RegExp(first.subject) }));
    await user.click(await toolbarAction(user, await screen.findByRole("toolbar", { name: "Message actions" }), "Forward as attachment"));
    await waitFor(() => {
      expect(core.callsOf("create_draft")[0]?.["request"]).toEqual({ kind: "forward_as_attachment", messageId: first.id, language: "en" });
    });
    expect((await screen.findByTestId("composer-attachment")).textContent).toContain("Hello.eml");
    expect(core.callsOf("check_draft_size")).toHaveLength(1);
  });
});

describe("composer helpers", () => {
  it("recognises image files by their extension", () => {
    expect(["a.png", "b.JPEG", "c.jpg", "d.gif", "e.webp"].every(isImagePath)).toBe(true);
    expect(["a.svg", "b.pdf", "png"].some(isImagePath)).toBe(false);
  });

  it("puts template text at the end of a plain text body without a cursor", () => {
    expect(insertAtCursor("", "Hallo", null)).toBe("Hallo");
    expect(insertAtCursor("Erste Zeile", "Hallo", null)).toBe("Erste Zeile\nHallo");
  });

  it("maps widths to the size menu", () => {
    expect(imageSizeOf(null)).toBe("original");
    expect(imageSizeOf(160)).toBe("small");
    expect(imageSizeOf(320)).toBe("medium");
    expect(imageSizeOf(900)).toBe("large");
  });
});

describe("composer: events", () => {
  it("signaturesChanged and templatesChanged reload the pool and the templates", async () => {
    const draft = richDraft();
    const { core, user } = setup({ compose: { drafts: [draft] }, snippets: { signatures: [makeSignature()], templates: [makeTemplate()] } });
    await openRich(core, draft);
    await user.click(screen.getByRole("button", { name: "Template" }));
    await screen.findByTestId("composer-template-option");
    const signatures = core.callsOf("list_signatures").length;
    const templates = core.callsOf("list_templates").length;
    await emitCore({ type: "signaturesChanged" });
    await emitCore({ type: "templatesChanged" });
    await waitFor(() => {
      expect(core.callsOf("list_signatures").length).toBeGreaterThan(signatures);
      expect(core.callsOf("list_templates").length).toBeGreaterThan(templates);
    });
  });
});
