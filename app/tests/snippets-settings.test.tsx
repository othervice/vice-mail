import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SignatureInput, TemplateInput } from "../src/services/ipc";
import { useUiStore } from "../src/stores/ui";
import { makeIdentity } from "./support/compose";
import { ipcError, makeAccount, makeMessages, mockCore, standardMailboxes, type Failure } from "./support/core";
import { dragFilesOver, dropFiles, fileTransfer } from "./support/fileDrop";
import { renderApp } from "./support/render";
import { makeSignature, makeTemplate, type SnippetsFakeOptions } from "./support/snippets";

const ADA = makeAccount({ writesEnabled: true });

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
  snippets?: SnippetsFakeOptions;
  failures?: Partial<Record<string, Failure>>;
}

async function openWriting({ snippets = {}, failures }: SetupOptions = {}) {
  const items = makeMessages(2);
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    compose: { identities: { [ADA.id]: [makeIdentity({ id: 1, accountId: ADA.id, email: ADA.email, name: "Ada" })] } },
    snippets,
    ...(failures && { failures }),
  });
  const rendered = renderApp();
  await screen.findByRole("button", { name: "New message" });
  act(() => {
    useUiStore.getState().showSettings("compose");
  });
  await screen.findByRole("heading", { name: "Signatures" });
  return { core, ...rendered };
}

const signatureSaves = (core: ReturnType<typeof mockCore>) => core.callsOf("save_signature").map((c) => c["signature"] as SignatureInput);
const templateSaves = (core: ReturnType<typeof mockCore>) => core.callsOf("save_template").map((c) => c["template"] as TemplateInput);

describe("signatures (Settings → Writing)", () => {
  it("shows empty states with the next step", async () => {
    await openWriting();
    expect(await screen.findByTestId("signatures-empty")).toBeTruthy();
    expect(await screen.findByTestId("templates-empty")).toBeTruthy();
    expect(screen.getByRole("button", { name: "New signature" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "New template" })).toBeTruthy();
  });

  it("creates a signature and says what sanitising changed", async () => {
    const { core, user } = await openWriting({ snippets: { notes: { imagesRemoved: 1, formattingSimplified: true } } });
    await user.click(await screen.findByRole("button", { name: "New signature" }));
    const editor = await screen.findByTestId("signature-editor");
    // A name is needed first.
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    expect(within(editor).getByText("Give it a name.")).toBeTruthy();
    expect(signatureSaves(core)).toEqual([]);
    await user.type(within(editor).getByLabelText("Name"), "Work");
    const body = await within(editor).findByRole("textbox", { name: "Signature" });
    await user.click(body);
    await user.type(body, "Ada Lovelace");
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(signatureSaves(core)).toHaveLength(1);
    });
    expect(signatureSaves(core)[0]).toMatchObject({ id: null, name: "Work" });
    expect(signatureSaves(core)[0]?.html).toContain("Ada Lovelace");
    const notes = await screen.findByTestId("signature-notes");
    expect(notes.textContent).toContain("1 image that was not added here was removed.");
    expect(notes.textContent).toContain("Colours, fonts and sizes were simplified");
    const list = await screen.findByTestId("signature-list");
    expect(within(list).getByText("Work")).toBeTruthy();
    expect(core.snippets.signatures.map((s) => s.name)).toEqual(["Work"]);
  });

  it("renames a signature and keeps its body", async () => {
    const { core, user } = await openWriting({ snippets: { signatures: [makeSignature()] } });
    await user.click(await screen.findByRole("button", { name: "Edit signature Work" }));
    const editor = await screen.findByTestId("signature-editor");
    const name = within(editor).getByLabelText("Name");
    await user.clear(name);
    await user.type(name, "Private");
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(signatureSaves(core)).toEqual([{ id: "sig-1", name: "Private", html: "<p>Ada Lovelace</p>" }]);
    });
    expect(await within(editor).findByText("Saved")).toBeTruthy();
    expect(within(screen.getByTestId("signature-list")).getByText("Private")).toBeTruthy();
  });

  it("saves a new signature before adding an image and puts the image into it as cid:", async () => {
    const picked = [{ contentId: "logo1@vicemail.test", fileName: "logo.png", mime: "image/png", size: 2048 }];
    const { core, user } = await openWriting({ snippets: { pickedSignatureImages: picked } });
    await user.click(await screen.findByRole("button", { name: "New signature" }));
    const editor = await screen.findByTestId("signature-editor");
    await user.type(within(editor).getByLabelText("Name"), "Logo");
    await within(editor).findByRole("textbox", { name: "Signature" });
    await user.click(within(editor).getByRole("button", { name: "Add image" }));
    await waitFor(() => {
      expect(core.callsOf("pick_signature_images")).toEqual([{ signatureId: "sig-1" }]);
    });
    // Created first: images belong to a stored signature.
    expect(signatureSaves(core)[0]).toMatchObject({ id: null, name: "Logo" });
    const images = await within(editor).findByTestId("signature-images");
    expect(within(images).getByText("logo.png")).toBeTruthy();
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(signatureSaves(core)).toHaveLength(2);
    });
    expect(signatureSaves(core)[1]).toMatchObject({ id: "sig-1", name: "Logo" });
    expect(signatureSaves(core)[1]?.html).toContain('src="cid:logo1@vicemail.test"');
    expect(signatureSaves(core)[1]?.html).not.toContain("data:");
  });

  it("image files dropped on the window while a signature is edited go into it", async () => {
    const { core, user } = await openWriting({ snippets: { signatures: [makeSignature()] } });
    await user.click(await screen.findByRole("button", { name: "Edit signature Work" }));
    const editor = await screen.findByTestId("signature-editor");
    const transfer = fileTransfer([new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47])], "logo.png", { type: "image/png" })]);
    dragFilesOver(editor, transfer);
    expect(await within(editor).findByTestId("signature-drop-hint")).toBeTruthy();
    expect(dropFiles(editor, transfer)).toBe(true);
    await waitFor(() => {
      expect(core.callsOf("add_uploaded_signature_images")).toEqual([{ signatureId: "sig-1", uploads: ["upload-1"] }]);
    });
    expect(core.callsOf("begin_file_upload")).toEqual([{ name: "logo.png", size: 4 }]);
    expect(within(await within(editor).findByTestId("signature-images")).getByText("logo.png")).toBeTruthy();
    expect(within(editor).queryByTestId("signature-drop-hint")).toBeNull();
  });

  it("removes an image of a signature", async () => {
    const image = { contentId: "logo@vicemail.test", fileName: "logo.png", mime: "image/png", size: 2048 };
    const signature = makeSignature({ html: '<p>Ada</p><p><img src="cid:logo@vicemail.test" alt=""></p>', images: [image] });
    const { core, user } = await openWriting({ snippets: { signatures: [signature] } });
    await user.click(await screen.findByRole("button", { name: "Edit signature Work" }));
    const editor = await screen.findByTestId("signature-editor");
    await user.click(within(editor).getByRole("button", { name: "Remove logo.png" }));
    await waitFor(() => {
      expect(core.callsOf("remove_signature_image")).toEqual([{ signatureId: "sig-1", contentId: "logo@vicemail.test" }]);
    });
    await waitFor(() => {
      expect(within(editor).queryByTestId("signature-images")).toBeNull();
    });
  });

  it("shows a refused image in place", async () => {
    const { user } = await openWriting({
      snippets: { signatures: [makeSignature()] },
      failures: { pick_signature_images: ipcError("attachment_too_large", "at most 8 images") },
    });
    await user.click(await screen.findByRole("button", { name: "Edit signature Work" }));
    const editor = await screen.findByTestId("signature-editor");
    await user.click(within(editor).getByRole("button", { name: "Add image" }));
    const error = await within(editor).findByTestId("signature-error");
    expect(error.textContent).toContain("The images of the signature could not be changed.");
  });

  it("deletes a signature only after confirming", async () => {
    const { core, user } = await openWriting({ snippets: { signatures: [makeSignature()] } });
    await user.click(await screen.findByRole("button", { name: "Delete signature Work" }));
    let dialog = await screen.findByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Keep" }));
    expect(core.callsOf("delete_signature")).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Delete signature Work" }));
    dialog = await screen.findByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(core.callsOf("delete_signature")).toEqual([{ signatureId: "sig-1" }]);
    });
    expect(await screen.findByTestId("signatures-empty")).toBeTruthy();
  });

  it("sets the default signatures of an address for new mails and replies", async () => {
    const signatures = [makeSignature(), makeSignature({ id: "sig-2", name: "Private", html: "<p>Ada</p>" })];
    // A deleted signature that is still a default counts as none.
    const { core, user } = await openWriting({ snippets: { signatures, defaults: { 1: { newSignature: "sig-gone", replySignature: null } } } });
    const defaults = await screen.findByTestId("signature-defaults");
    const forNew = within(defaults).getByRole<HTMLSelectElement>("combobox", { name: "New messages" });
    const forReplies = within(defaults).getByRole<HTMLSelectElement>("combobox", { name: "Replies and forwards" });
    await waitFor(() => {
      expect(forNew.disabled).toBe(false);
    });
    expect(forNew.value).toBe("");
    await user.selectOptions(forNew, "Work");
    await waitFor(() => {
      expect(core.callsOf("set_identity_signature_defaults")).toEqual([{ identityId: 1, newSignature: "sig-1", replySignature: null }]);
    });
    await user.selectOptions(forReplies, "Private");
    await waitFor(() => {
      expect(core.callsOf("set_identity_signature_defaults").at(-1)).toEqual({ identityId: 1, newSignature: "sig-1", replySignature: "sig-2" });
    });
    expect(core.snippets.defaultsOf(1)).toEqual({ identityId: 1, newSignature: "sig-1", replySignature: "sig-2" });
    await waitFor(() => {
      expect(forReplies.value).toBe("sig-2");
    });
  });

  it("shows a failed save of the defaults in place", async () => {
    const { user } = await openWriting({
      snippets: { signatures: [makeSignature()] },
      failures: { set_identity_signature_defaults: ipcError("not_found") },
    });
    const defaults = await screen.findByTestId("signature-defaults");
    const forNew = within(defaults).getByRole<HTMLSelectElement>("combobox", { name: "New messages" });
    await waitFor(() => {
      expect(forNew.disabled).toBe(false);
    });
    await user.selectOptions(forNew, "Work");
    const error = await within(defaults).findByTestId("signature-defaults-error");
    expect(error.textContent).toContain("The default signature could not be saved.");
  });
});

describe("templates (Settings → Writing)", () => {
  it("creates a template with placeholders and warns about unknown ones", async () => {
    const { core, user } = await openWriting();
    await user.click(await screen.findByRole("button", { name: "New template" }));
    const editor = await screen.findByTestId("template-editor");
    await user.type(within(editor).getByLabelText("Name"), "Thanks");
    fireEvent.change(within(editor).getByLabelText("Subject"), { target: { value: "Hi {{nickname}}" } });
    const body = await within(editor).findByRole("textbox", { name: "Text" });
    await user.click(body);
    await user.type(body, "Hello ");
    await user.click(within(editor).getByRole("button", { name: "Insert placeholder: Recipient's first name" }));
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(templateSaves(core)).toHaveLength(1);
    });
    expect(templateSaves(core)[0]).toMatchObject({ id: null, name: "Thanks", subject: "Hi {{nickname}}" });
    expect(templateSaves(core)[0]?.html).toContain("Hello");
    expect(templateSaves(core)[0]?.html).toContain("{{first_name}}");
    const unknown = await within(editor).findByTestId("template-unknown");
    expect(unknown.textContent).toContain("{{nickname}} cannot be filled and stay as written.");
    const list = await screen.findByTestId("template-list");
    expect(within(list).getByText("Thanks")).toBeTruthy();
    expect(within(list).getByTestId("template-item-unknown").textContent).toBe("Unknown placeholders: {{nickname}}");
  });

  it("edits a template and keeps its body", async () => {
    const { core, user } = await openWriting({ snippets: { templates: [makeTemplate()] } });
    await user.click(await screen.findByRole("button", { name: "Edit template Thank you" }));
    const editor = await screen.findByTestId("template-editor");
    fireEvent.change(within(editor).getByLabelText("Subject"), { target: { value: "Thank you, {{first_name|there}}" } });
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(templateSaves(core)).toEqual([
        { id: "tpl-1", name: "Thank you", subject: "Thank you, {{first_name|there}}", html: "<p>Hello {{first_name}}</p>" },
      ]);
    });
    expect(within(editor).queryByTestId("template-unknown")).toBeNull();
  });

  it("shows a refused template in place", async () => {
    const { user } = await openWriting({
      snippets: { templates: [makeTemplate()] },
      failures: { save_template: ipcError("invalid_argument", "templates contain no images") },
    });
    await user.click(await screen.findByRole("button", { name: "Edit template Thank you" }));
    const editor = await screen.findByTestId("template-editor");
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    const error = await within(editor).findByTestId("template-error");
    expect(error.textContent).toContain("The template could not be saved.");
  });

  it("deletes a template only after confirming", async () => {
    const { core, user } = await openWriting({ snippets: { templates: [makeTemplate()] } });
    await user.click(await screen.findByRole("button", { name: "Delete template Thank you" }));
    const dialog = await screen.findByRole("alertdialog");
    expect(within(dialog).getByText("Delete the template “Thank you”?")).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(core.callsOf("delete_template")).toEqual([{ templateId: "tpl-1" }]);
    });
    expect(await screen.findByTestId("templates-empty")).toBeTruthy();
  });
});
