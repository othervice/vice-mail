import { act, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";
import { clampCrop, initialCrop, zoomCrop } from "../src/features/contacts/PictureEditor";
import type { ContactEntry, MessageListItem } from "../src/services/ipc";
import { useSearchStore } from "../src/stores/search";
import { useUiStore } from "../src/stores/ui";
import { makeContact, makePictureSource } from "./support/contacts";
import { ipcError, makeAccount, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";

/*
 * Stage 17, block B (decision 0050): Contacts as a view of their own. Pictures (chosen in the
 * system dialog, cut to a square with mouse or keyboard, shown wherever initials were), search,
 * the detail with note and recent mail, "Write mail", "Show all mail" and "Mark as spam" with the
 * move to Junk, read-only accounts and undo.
 */

const ADA = makeAccount({ writesEnabled: true });
const GRACE = makeContact({ email: "grace@example.org", name: "Grace Hopper", sent: 12, received: 30, lastUsed: Date.UTC(2026, 10, 20, 12) });
const ALAN = makeContact({ email: "alan@example.org", name: "Alan Turing", sent: 3, received: 1, lastUsed: Date.UTC(2026, 9, 2, 12) });
const PUSHY = makeContact({ email: "offers@spam.example", name: "Pushy", received: 4 });
const DAEMON = makeContact({ email: "mailer-daemon@example.org", name: "Mail Delivery System", received: 2, kind: "system" });
const WITH_PICTURE: ContactEntry = { ...GRACE, picture: 7 };

type User = ReturnType<typeof renderApp>["user"];

async function openView(user: User) {
  await user.click(await screen.findByRole("button", { name: "Contacts" }));
  const view = await screen.findByTestId("contacts-view");
  await within(view).findByRole("listbox", { name: "Contacts" });
  return view;
}

async function seriousFindings(): Promise<string[]> {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  const results = await axe.run(document.body, { resultTypes: ["violations"], iframes: false, rules: { "color-contrast": { enabled: false } } });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}

const crop = () => screen.getByTestId("picture-crop-area").dataset["crop"];

describe("the cropper's square", () => {
  it("starts as the largest square in the middle and stays inside the image", () => {
    expect(initialCrop(800, 600)).toEqual({ x: 100, y: 0, size: 600 });
    expect(clampCrop({ x: -50, y: 700, size: 2000 }, 800, 600)).toEqual({ x: 0, y: 0, size: 600 });
    expect(clampCrop({ x: 790, y: 10, size: 10 }, 800, 600)).toEqual({ x: 725, y: 10, size: 75 });
    // Zooming keeps the centre: twice as close is half the side, around the same point.
    expect(zoomCrop({ x: 100, y: 0, size: 600 }, 2, 800, 600)).toEqual({ x: 250, y: 150, size: 300 });
    expect(zoomCrop({ x: 100, y: 0, size: 600 }, 100, 800, 600).size).toBe(75);
  });
});

describe("Contacts view: pictures", () => {
  it("chooses a picture, cuts it with the keyboard and saves it; the list shows it", async () => {
    const core = mockCore({ accounts: [ADA], mailboxes: standardMailboxes(), contacts: { contacts: [GRACE, ALAN], picked: makePictureSource() } });
    const { user } = renderApp();
    const view = await openView(user);
    expect(within(view).getAllByTestId("avatar")[0]?.dataset["picture"]).toBeUndefined();
    await user.click(within(view).getByRole("button", { name: "Choose picture…" }));
    const editor = await within(view).findByTestId("picture-editor");
    expect(core.callsOf("pick_contact_picture")).toHaveLength(1);
    // The image is the helper's encoding (a JPEG data URL), shown through an SVG view box.
    expect(editor.querySelector("image")?.getAttribute("href")).toMatch(/^data:image\/jpeg;base64,/);
    const area = within(editor).getByRole("group", { name: "Picture of Grace Hopper" });
    expect(crop()).toBe("100,0,600");
    act(() => {
      area.focus();
    });
    await user.keyboard("{+}{ArrowRight}{ArrowDown}");
    expect(crop()).toBe("184,84,480");
    await user.keyboard("{Shift>}{ArrowLeft}{/Shift}");
    expect(crop()).toBe("88,84,480");
    // Back to the whole height: the square cannot leave the image, so it moves up.
    await user.keyboard("{-}");
    expect(crop()).toBe("28,0,600");
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(within(view).queryByTestId("picture-editor")).toBeNull();
    });
    expect(core.contacts.crops).toEqual([{ email: "grace@example.org", token: "token-1", crop: { x: 28, y: 0, size: 600 } }]);
    // The new picture in the list and in the contact's page (from the core, never the file).
    await waitFor(() => {
      const row = within(view).getAllByTestId("contact-row")[0];
      expect(row?.querySelector('[data-testid="avatar-picture"]')?.getAttribute("src")).toBe("data:image/jpeg;base64,/9j/graceexampleorg");
    });
    const page = within(view).getByTestId("contact-picture-area");
    await waitFor(() => {
      expect(within(page).getByTestId("avatar-picture")).toBeTruthy();
    });
    expect(within(page).getByRole("button", { name: "Change picture…" })).toBeTruthy();
    await user.click(within(page).getByRole("button", { name: "Remove picture" }));
    await waitFor(() => {
      expect(within(view).queryByTestId("avatar-picture")).toBeNull();
    });
    expect(core.callsOf("remove_contact_picture")).toEqual([{ email: "grace@example.org" }]);
  });

  it("cancels with Escape; a cancelled dialog changes nothing; a refused file says why", async () => {
    const core = mockCore({ accounts: [ADA], contacts: { contacts: [GRACE], picked: makePictureSource() } });
    const { user } = renderApp();
    const view = await openView(user);
    await user.click(within(view).getByRole("button", { name: "Choose picture…" }));
    await within(view).findByTestId("picture-editor");
    expect(await seriousFindings()).toEqual([]);
    act(() => {
      screen.getByTestId("picture-crop-area").focus();
    });
    await user.keyboard("{Escape}");
    expect(within(view).queryByTestId("picture-editor")).toBeNull();
    expect(core.contacts.crops).toEqual([]);

    const cancelled = mockCore({ accounts: [ADA], contacts: { contacts: [GRACE], picked: null } });
    await user.click(within(view).getByRole("button", { name: "Choose picture…" }));
    await waitFor(() => {
      expect(cancelled.callsOf("pick_contact_picture")).toHaveLength(1);
    });
    expect(within(view).queryByTestId("picture-editor")).toBeNull();

    mockCore({ accounts: [ADA], contacts: { contacts: [GRACE], picked: ipcError("picture_damaged", "the decoder failed") } });
    await user.click(within(view).getByRole("button", { name: "Choose picture…" }));
    const error = await within(view).findByTestId("picture-error");
    expect(error.dataset["errorCode"]).toBe("picture_damaged");
    expect(error.textContent).toContain("The picture could not be used.");
    expect(error.textContent).toContain("The picture could not be read.");
  });

  it("shows the picture wherever initials were: message list, reading pane and the composer's chips", async () => {
    // The list virtualizer needs a viewport height (jsdom has no layout).
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
    const message: MessageListItem = makeMessage({ subject: "Compilers", from: { name: "Grace Hopper", email: "Grace@Example.org" } });
    mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: [message] },
      views: { [message.id]: makeView(message) },
      contacts: { contacts: [WITH_PICTURE] },
    });
    const { user } = renderApp();
    const row = (await screen.findAllByRole("option"))[0] ?? document.body;
    await waitFor(() => {
      expect(within(row).getByTestId("avatar-picture").getAttribute("src")).toBe("data:image/jpeg;base64,/9j/graceexampleorg");
    });
    await user.click(row);
    const pane = await screen.findByRole("region", { name: "Message" });
    await waitFor(() => {
      expect(within(pane).getByTestId("avatar-picture")).toBeTruthy();
    });
    // "Write mail" from the contact: the composer's chip carries the picture too.
    const view = await openView(user);
    await user.click(within(view).getByRole("button", { name: "Write mail" }));
    const chip = await screen.findByTestId("recipient-chip");
    expect(chip.textContent).toContain("Grace Hopper");
    await waitFor(() => {
      expect(within(chip).getByTestId("avatar-picture")).toBeTruthy();
    });
  });
});

describe("Contacts view: search, keyboard and the contact's page", () => {
  it("moves through the list with the arrow keys and goes to the contact with Enter", async () => {
    mockCore({ accounts: [ADA], contacts: { contacts: [GRACE, ALAN] } });
    const { user } = renderApp();
    const view = await openView(user);
    const search = within(view).getByLabelText("Search contacts");
    act(() => {
      search.focus();
    });
    await user.keyboard("{ArrowDown}");
    const list = within(view).getByRole("listbox", { name: "Contacts" });
    expect(document.activeElement).toBe(list);
    await user.keyboard("{ArrowDown}");
    expect(within(view).getByTestId("contact-detail-name").textContent).toBe("Alan Turing");
    expect(list.getAttribute("aria-activedescendant")).toBe(within(list).getAllByRole("option")[1]?.id);
    expect(within(list).getAllByRole("option").map((o) => o.getAttribute("aria-selected"))).toEqual(["false", "true"]);
    await user.keyboard("{Home}");
    expect(within(view).getByTestId("contact-detail-name").textContent).toBe("Grace Hopper");
    await user.keyboard("{Enter}");
    expect(document.activeElement).toBe(within(view).getByTestId("contact-detail-name"));
  });

  it("lists list and system addresses only on request, marked", async () => {
    const core = mockCore({ accounts: [ADA], contacts: { contacts: [GRACE, DAEMON] } });
    const { user } = renderApp();
    const view = await openView(user);
    expect(within(view).getAllByTestId("contact-name").map((n) => n.textContent)).toEqual(["Grace Hopper"]);
    await user.click(within(view).getByRole("checkbox", { name: "Show lists and system addresses" }));
    await waitFor(() => {
      expect(within(view).getAllByTestId("contact-row")).toHaveLength(2);
    });
    expect(core.callsOf("list_contacts").at(-1)?.["filter"]).toBe("all");
    const daemon = within(view).getAllByTestId("contact-row")[1];
    expect(within(daemon ?? view).getByTestId("contact-kind").textContent).toBe("System");
  });

  it("keeps a note, lists recent mail and opens it, writes mail and shows all mail from the contact", async () => {
    const recent = makeMessage({ id: 55, subject: "Re: compilers", accountId: ADA.id });
    const core = mockCore({ accounts: [ADA], mailboxes: standardMailboxes(), contacts: { contacts: [GRACE], messages: { "grace@example.org": [recent] } } });
    const { user } = renderApp();
    const view = await openView(user);
    const note = within(view).getByRole("textbox", { name: "Note" });
    await user.type(note, "Met at the fair");
    await user.click(within(view).getByRole("button", { name: "Save note" }));
    await waitFor(() => {
      expect(core.callsOf("set_contact_note")).toEqual([{ email: "grace@example.org", note: "Met at the fair" }]);
    });
    expect(await within(view).findByText("Saved.")).toBeTruthy();

    const mail = within(view).getByRole("list", { name: "Recent mail" });
    await user.click(within(mail).getByRole("button", { name: /Re: compilers/ }));
    await waitFor(() => {
      expect(core.callsOf("open_message_window")).toEqual([{ accountId: ADA.id, messageId: 55 }]);
    });

    await user.click(within(view).getByRole("button", { name: "Write mail" }));
    await waitFor(() => {
      expect(core.callsOf("create_draft")[0]?.["request"]).toMatchObject({
        kind: "new",
        accountId: ADA.id,
        to: [{ name: "Grace Hopper", email: "grace@example.org" }],
      });
    });

    await user.click(screen.getByRole("button", { name: "Show all mail" }));
    expect(useUiStore.getState().view).toEqual({ kind: "mail", mailView: "inbox" });
    expect(useSearchStore.getState()).toMatchObject({ open: true, query: "from:grace@example.org", submitted: true, scope: "all" });
  });
});

describe("Contacts view: mark as spam", () => {
  it("blocks, offers to move the existing mail to Junk, says what stayed, and undoes both", async () => {
    const core = mockCore({
      accounts: [ADA],
      contacts: {
        contacts: [PUSHY],
        spamMail: {
          "offers@spam.example": [
            { accountId: ADA.id, count: 3, writesEnabled: true },
            { accountId: makeAccount({ id: "00000000-0000-4000-8000-0000000000b2" }).id, count: 2, writesEnabled: false },
          ],
        },
      },
    });
    const { user } = renderApp();
    const view = await openView(user);
    await user.click(within(view).getByRole("button", { name: "Mark as spam" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Move 3 messages to Junk too?" });
    expect(dialog.textContent).toContain("Pushy is marked as spam: new mail from it goes to Junk.");
    expect(dialog.textContent).toContain("2 messages in read-only accounts stay where they are.");
    expect(await seriousFindings()).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "Move to Junk" }));
    const notice = await screen.findByText("Pushy is marked as spam; 3 messages moved to Junk.");
    expect(screen.getByText("2 messages in read-only accounts stayed where they are.")).toBeTruthy();
    expect(core.callsOf("move_contact_mail_to_junk")).toEqual([{ email: "offers@spam.example" }]);
    expect(core.contacts.blocked).toEqual(["offers@spam.example"]);
    await waitFor(() => {
      expect(within(view).getByTestId("contact-detail").textContent).toContain("Spam");
    });
    expect(within(view).getByRole("button", { name: "Not spam" })).toBeTruthy();

    const container = notice.closest('[data-testid="notice"]') ?? document.body;
    await user.click(within(container as HTMLElement).getByRole("button", { name: "Undo" }));
    await waitFor(() => {
      expect(core.contacts.undos).toEqual([{ addresses: ["offers@spam.example"], batches: [501] }]);
    });
    expect(core.contacts.blocked).toEqual([]);
    await waitFor(() => {
      expect(within(view).getByRole("button", { name: "Mark as spam" })).toBeTruthy();
    });
  });

  it("asks nothing when only read-only accounts have mail from it, and says it stays", async () => {
    const core = mockCore({
      accounts: [makeAccount({ writesEnabled: false })],
      contacts: { contacts: [PUSHY], spamMail: { "offers@spam.example": [{ accountId: ADA.id, count: 1, writesEnabled: false }] } },
    });
    const { user } = renderApp();
    const view = await openView(user);
    await user.click(within(view).getByRole("button", { name: "Mark as spam" }));
    expect(await screen.findByText("Pushy is marked as spam. New mail from it goes to Junk.")).toBeTruthy();
    expect(screen.getByText("1 message in a read-only account stayed where it is.")).toBeTruthy();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(core.callsOf("move_contact_mail_to_junk")).toEqual([]);
  });

  it("leaves the mail where it is on request; “Not spam” unblocks", async () => {
    const core = mockCore({
      accounts: [ADA],
      contacts: { contacts: [PUSHY], spamMail: { "offers@spam.example": [{ accountId: ADA.id, count: 4, writesEnabled: true }] } },
    });
    const { user } = renderApp();
    const view = await openView(user);
    await user.click(within(view).getByRole("button", { name: "Mark as spam" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Move 4 messages to Junk too?" });
    await user.click(within(dialog).getByRole("button", { name: "Leave it" }));
    expect(await screen.findByText("Pushy is marked as spam. New mail from it goes to Junk.")).toBeTruthy();
    expect(core.callsOf("move_contact_mail_to_junk")).toEqual([]);
    await user.click(await within(view).findByRole("button", { name: "Not spam" }));
    await waitFor(() => {
      expect(core.contacts.undos).toEqual([{ addresses: ["offers@spam.example"], batches: [] }]);
    });
    expect(await screen.findByText("Pushy is no longer marked as spam.")).toBeTruthy();
  });

  it("shows a refusal of the core in place", async () => {
    mockCore({ accounts: [ADA], contacts: { contacts: [PUSHY] }, failures: { mark_contact_spam: ipcError("database", "locked") } });
    const { user } = renderApp();
    const view = await openView(user);
    await user.click(within(view).getByRole("button", { name: "Mark as spam" }));
    const error = await within(view).findByTestId("contact-spam-error");
    expect(error.dataset["errorCode"]).toBe("database");
    expect(error.textContent).toContain("The contact could not be marked as spam.");
  });
});

describe("Contacts view in German", () => {
  it("speaks German (informal)", async () => {
    mockCore({
      accounts: [ADA],
      preferences: { theme: "light", language: "de" },
      contacts: { contacts: [PUSHY], picked: makePictureSource(), spamMail: { "offers@spam.example": [{ accountId: ADA.id, count: 2, writesEnabled: true }] } },
    });
    const { user } = renderApp("de");
    await user.click(await screen.findByRole("button", { name: "Kontakte" }));
    const view = await screen.findByTestId("contacts-view");
    await within(view).findByRole("listbox", { name: "Kontakte" });
    expect(within(view).getByLabelText("Kontakte durchsuchen")).toBeTruthy();
    expect(within(view).getByRole("checkbox", { name: "Listen und Systemadressen zeigen" })).toBeTruthy();
    expect(within(view).getByRole("button", { name: "Mail schreiben" })).toBeTruthy();
    expect(within(view).getByRole("button", { name: "Alle Mails anzeigen" })).toBeTruthy();
    await user.click(within(view).getByRole("button", { name: "Bild wählen\u00a0…" }));
    expect(await within(view).findByText("Ausschnitt wählen")).toBeTruthy();
    expect(within(view).getByRole("button", { name: "Bild speichern" })).toBeTruthy();
    await user.click(within(view).getByRole("button", { name: "Abbrechen" }));
    await user.click(within(view).getByRole("button", { name: "Als Spam markieren" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Auch 2 vorhandene Mails in den Spam-Ordner verschieben?" });
    await user.click(within(dialog).getByRole("button", { name: "In den Spam-Ordner" }));
    expect(await screen.findByText("Pushy ist als Spam markiert; 2 Mails sind im Spam-Ordner.")).toBeTruthy();
  });
});
