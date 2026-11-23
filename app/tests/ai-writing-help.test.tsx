import { act, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Editor } from "@tiptap/react";
import type { AiWritingTask, DraftUpdate } from "../src/services/ipc";
import { CLOUD_PROFILE_ID, makeAiProfile, makeCloudProfile, makeTransfer, type AiFakeOptions } from "./support/ai";
import { makeDraft } from "./support/compose";
import { makeAccount, makeMessages, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";

const ADA = makeAccount();
const LOCAL = makeAiProfile();

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

/** The fake model: upper case for "improve", and says which task it got otherwise. */
function writing(passage: string, task: AiWritingTask) {
  const text =
    task.type === "improve"
      ? passage.toUpperCase()
      : task.type === "tone"
        ? `${passage} (${task.tone})`
        : task.type === "translate"
          ? `${passage} [${task.language}]`
          : `<b>${passage.slice(0, 3)}</b>`;
  return { text, transfer: makeTransfer({ function: "writing_help", mails: 0, bytes: passage.length }) };
}

function setup(ai: AiFakeOptions = {}, extra: Parameters<typeof mockCore>[0] = {}) {
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: makeMessages(2) },
    ai: { enabled: true, profiles: [LOCAL], functions: { writing_help: LOCAL.id }, writing, ...ai },
    ...extra,
  });
  return { core, ...renderApp() };
}

async function openNew(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("button", { name: "New message" }));
  await screen.findByLabelText("Subject");
  return screen.getByTestId("composer");
}

async function openHelp(user: ReturnType<typeof renderApp>["user"], composer: HTMLElement) {
  const toggle = await within(composer).findByRole("button", { name: "Writing help" });
  await user.click(toggle);
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  return within(composer).getByRole("toolbar", { name: "Writing help" });
}

async function typeAndSelect(user: ReturnType<typeof renderApp>["user"], composer: HTMLElement, text: string, start: number, end: number) {
  const body = within(composer).getByLabelText<HTMLTextAreaElement>("Message");
  await user.type(body, text);
  body.setSelectionRange(start, end);
  return body;
}

const saves = (core: ReturnType<typeof mockCore>) => core.callsOf("save_draft").map((c) => c["update"] as DraftUpdate);

describe("writing help in the composer", () => {
  it("improves the selection with a preview, and only Apply changes the draft", async () => {
    const { core, user } = setup();
    const composer = await openNew(user);
    const body = await typeAndSelect(user, composer, "Hello world, see you", 6, 11);
    const tools = await openHelp(user, composer);
    await user.click(within(tools).getByRole("button", { name: "Improve" }));

    const preview = await within(composer).findByRole("region", { name: "Suggestion" });
    expect(core.callsOf("ai_writing_help")).toEqual([{ requestId: expect.any(String) as string, passage: "world", task: { type: "improve" } }]);
    expect(within(preview).getByTestId("ai-writing-before").textContent).toBe("world");
    expect(within(preview).getByTestId("ai-writing-after").textContent).toBe("WORLD");
    expect(within(preview).getByTestId("ai-transfer-summary").textContent).toContain("of text, no mail, to localhost:11434");
    expect(body.value).toBe("Hello world, see you");

    await user.click(within(preview).getByRole("button", { name: "Apply" }));
    expect(body.value).toBe("Hello WORLD, see you");
    expect(within(composer).queryByRole("region", { name: "Suggestion" })).toBeNull();
    // Saved like typing.
    await user.click(within(composer).getByLabelText("Subject"));
    await waitFor(() => {
      expect(saves(core).at(-1)?.bodyText).toBe("Hello WORLD, see you");
    });
  });

  it("discards a suggestion without touching the text, and refuses to apply after the text changed", async () => {
    const { user } = setup();
    const composer = await openNew(user);
    const body = await typeAndSelect(user, composer, "Dear team", 0, 4);
    const tools = await openHelp(user, composer);
    await user.click(within(tools).getByRole("button", { name: "Improve" }));
    let preview = await within(composer).findByRole("region", { name: "Suggestion" });
    await user.click(within(preview).getByRole("button", { name: "Discard" }));
    expect(body.value).toBe("Dear team");
    expect(within(composer).queryByRole("region", { name: "Suggestion" })).toBeNull();

    body.setSelectionRange(0, 4);
    await user.click(within(tools).getByRole("button", { name: "Improve" }));
    preview = await within(composer).findByRole("region", { name: "Suggestion" });
    // The user edits the selected words in the meantime.
    await user.clear(body);
    await user.type(body, "Hi team");
    await user.click(within(preview).getByRole("button", { name: "Apply" }));
    expect(within(composer).getByRole("alert").textContent).toBe("The text changed in the meantime. Select it again and ask anew.");
    expect(body.value).toBe("Hi team");
  });

  it("sends tone and translation tasks, and nothing without a selection", async () => {
    const { core, user } = setup();
    const composer = await openNew(user);
    const body = await typeAndSelect(user, composer, "Thanks", 0, 0);
    const tools = await openHelp(user, composer);
    await user.click(within(tools).getByRole("button", { name: "Improve" }));
    expect(within(composer).getByRole("alert").textContent).toBe("Select the text you want help with first.");
    expect(core.callsOf("ai_writing_help")).toEqual([]);

    body.setSelectionRange(0, 6);
    await user.selectOptions(within(tools).getByRole("combobox", { name: "Tone" }), "formal");
    await user.click(within(tools).getByRole("button", { name: "Change tone" }));
    await within(composer).findByRole("region", { name: "Suggestion" });
    await user.click(within(composer).getByRole("button", { name: "Discard" }));
    body.setSelectionRange(0, 6);
    await user.selectOptions(within(tools).getByRole("combobox", { name: "Target language" }), "de");
    await user.click(within(tools).getByRole("button", { name: "Translate" }));
    await within(composer).findByRole("region", { name: "Suggestion" });
    expect(core.callsOf("ai_writing_help").map((c) => c["task"])).toEqual([
      { type: "tone", tone: "formal" },
      { type: "translate", language: "de" },
    ]);
  });

  it("asks for consent before the selection goes to a cloud provider", async () => {
    const { core, user } = setup({ profiles: [makeCloudProfile()], functions: { writing_help: CLOUD_PROFILE_ID } });
    const composer = await openNew(user);
    await typeAndSelect(user, composer, "Thanks a lot", 0, 6);
    const tools = await openHelp(user, composer);
    await user.click(within(tools).getByRole("button", { name: "Improve" }));
    const consent = await within(composer).findByRole("group", { name: "Before the first request to OpenAI" });
    expect(within(consent).getByTestId("ai-consent-what").textContent).toBe(
      "What is sent: only the text you selected in your message. No mail from your accounts.",
    );
    expect(core.callsOf("ai_writing_help")).toEqual([]);
    await user.click(within(consent).getByRole("button", { name: "Agree and send" }));
    await within(composer).findByRole("region", { name: "Suggestion" });
    expect(core.callsOf("ai_writing_help")).toHaveLength(1);
  });

  it("is not in the composer while the writing help has no provider", async () => {
    const { core, user } = setup({ functions: { summary: LOCAL.id } });
    const composer = await openNew(user);
    await waitFor(() => {
      expect(core.callsOf("get_ai_availability").length).toBeGreaterThan(0);
    });
    expect(within(composer).queryByRole("button", { name: "Writing help" })).toBeNull();
  });

  it("cancels a running request when the composer closes", async () => {
    const { core, user } = setup({ hold: true });
    const composer = await openNew(user);
    await typeAndSelect(user, composer, "Thanks a lot", 0, 6);
    const tools = await openHelp(user, composer);
    await user.click(within(tools).getByRole("button", { name: "Improve" }));
    await within(composer).findByTestId("ai-progress");
    await user.click(within(composer).getByTestId("composer-close"));
    await waitFor(() => {
      expect(core.callsOf("cancel_ai_request")).toHaveLength(1);
    });
  });
});

describe("writing help in the rich text editor", () => {
  // ProseMirror measures the selection; jsdom has no layout.
  beforeEach(() => {
    const rects = () => Object.assign([], { item: () => null }) as unknown as DOMRectList;
    document.elementFromPoint = () => null;
    Range.prototype.getClientRects = rects;
    Range.prototype.getBoundingClientRect = () => new DOMRect();
    (Text.prototype as unknown as { getClientRects: () => DOMRectList }).getClientRects = rects;
  });

  it("replaces only the selection, as text: markup in the answer is never parsed", async () => {
    const rich = makeDraft({ accountId: ADA.id, format: "html", bodyHtml: "<p>Hello Grace</p>" });
    const { user } = setup({}, { compose: { drafts: [rich] } });
    await user.click(await screen.findByRole("button", { name: "Open the newest" }));
    const editorNode = await screen.findByTestId("composer-rich-body");
    const editor = (editorNode as HTMLElement & { editor?: Editor }).editor;
    if (!editor) throw new Error("no editor");
    // "Grace" (positions inside the first paragraph).
    act(() => {
      editor.commands.setTextSelection({ from: 7, to: 12 });
    });
    const composer = screen.getByTestId("composer");
    const tools = await openHelp(user, composer);
    await user.click(within(tools).getByRole("button", { name: "Shorten" }));
    const preview = await within(composer).findByRole("region", { name: "Suggestion" });
    expect(within(preview).getByTestId("ai-writing-before").textContent).toBe("Grace");
    await user.click(within(preview).getByRole("button", { name: "Apply" }));
    expect(editorNode.textContent).toBe("Hello <b>Gra</b>");
    expect(editorNode.querySelector("b")).toBeNull();
  });
});
