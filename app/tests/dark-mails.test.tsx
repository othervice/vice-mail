// Stage 21, item 8 (decision 0066): mails in the dark theme. The core adapts the colours of an
// HTML mail when the reading pane asks for the dark scheme; "Original colours" asks for the light
// one (the mail as written, on its white canvas) and back; the setting "always on a white page"
// is stored with the reading preferences; printing shows the original colours first.
import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BodyFormat, MailColors, MailScheme, MessageView } from "../src/services/ipc";
import { htmlBody, makeAccount, makeMessage, makeView, mockCore, standardMailboxes, textBody } from "./support/core";
import { renderApp } from "./support/render";

const ACCOUNT = makeAccount();
const NEWSLETTER = makeMessage({
  subject: "Autumn offers",
  from: { name: "Shop", email: "news@shop.example" },
});

/** The list virtualizer needs a viewport height (jsdom has no layout). */
beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

/** As the core answers: adapted for the dark scheme, as written for the light one. */
function views(darkColors: MailColors = "adapted") {
  return (_format: BodyFormat, scheme: MailScheme): MessageView =>
    makeView(NEWSLETTER, {
      body: htmlBody(scheme === "dark" ? "<p style='color:#e6edf3'>Offers</p>" : "<p style='color:#333'>Offers</p>", {
        colors: scheme === "dark" ? darkColors : "original",
      }),
    });
}

function setup(theme: "light" | "dark", darkColors: MailColors = "adapted") {
  return mockCore({
    accounts: [ACCOUNT],
    mailboxes: standardMailboxes(),
    messages: { 1: [NEWSLETTER] },
    views: { [NEWSLETTER.id]: views(darkColors) },
    preferences: { theme, language: "en" },
  });
}

async function openNewsletter(user: ReturnType<typeof renderApp>["user"]) {
  const [row] = await screen.findAllByRole("option");
  if (!row) throw new Error("no row");
  await user.click(row);
  return screen.findByRole("region", { name: "Message" });
}

const frameIn = (pane: HTMLElement) => within(pane).getByTitle("Content of the message “Autumn offers”");

describe("mails in the dark theme", () => {
  it("asks the core for the dark scheme and lays the frame on the dark reading surface", async () => {
    const core = setup("dark");
    const { user } = renderApp();
    const pane = await openNewsletter(user);

    await waitFor(() => {
      expect(frameIn(pane).getAttribute("data-colors")).toBe("adapted");
    });
    expect(core.callsOf("get_message")).toEqual([{ messageId: NEWSLETTER.id, format: "auto", scheme: "dark" }]);
    const frame = frameIn(pane);
    expect(frame.className).toContain("scheme-dark");
    // Stage 23 (decision 0084): on the reading pane's surface (no card any more).
    expect(frame.className).toMatch(/\bbg-surface\b/);
    expect(frame.className).not.toContain("bg-mail-canvas");
    const toggle = within(pane).getByRole("button", { name: "Original colours" });
    expect(toggle.getAttribute("aria-pressed")).toBe("false");
  });

  it("shows one message in its original colours on the white canvas and goes back", async () => {
    const core = setup("dark");
    const { user } = renderApp();
    const pane = await openNewsletter(user);
    const toggle = await within(pane).findByRole("button", { name: "Original colours" });

    await user.click(toggle);
    await waitFor(() => {
      expect(frameIn(pane).getAttribute("data-colors")).toBe("original");
    });
    expect(frameIn(pane).className).toContain("bg-mail-canvas");
    expect(frameIn(pane).className).toContain("scheme-light");
    expect(within(pane).getByRole("button", { name: "Original colours" }).getAttribute("aria-pressed")).toBe("true");
    expect(core.callsOf("get_message").at(-1)).toEqual({ messageId: NEWSLETTER.id, format: "auto", scheme: "light" });

    await user.click(within(pane).getByRole("button", { name: "Original colours" }));
    await waitFor(() => {
      expect(frameIn(pane).getAttribute("data-colors")).toBe("adapted");
    });
    expect(within(pane).getByRole("button", { name: "Original colours" }).getAttribute("aria-pressed")).toBe("false");
  });

  it("asks for the light scheme in the light theme and offers no toggle", async () => {
    const core = setup("light");
    const { user } = renderApp();
    const pane = await openNewsletter(user);

    await waitFor(() => {
      expect(frameIn(pane).getAttribute("data-colors")).toBe("original");
    });
    expect(core.callsOf("get_message")).toEqual([{ messageId: NEWSLETTER.id, format: "auto", scheme: "light" }]);
    expect(within(pane).queryByRole("button", { name: "Original colours" })).toBeNull();
  });

  it("offers no toggle when the core shows the mail as written anyway (setting, only light)", async () => {
    setup("dark", "original");
    const { user } = renderApp();
    const pane = await openNewsletter(user);
    await waitFor(() => {
      expect(frameIn(pane).getAttribute("data-colors")).toBe("original");
    });
    expect(within(pane).queryByRole("button", { name: "Original colours" })).toBeNull();
  });

  it("leaves plain-text mail in the theme without a toggle", async () => {
    mockCore({
      accounts: [ACCOUNT],
      mailboxes: standardMailboxes(),
      messages: { 1: [NEWSLETTER] },
      views: { [NEWSLETTER.id]: makeView(NEWSLETTER, { body: textBody([{ kind: "text", text: "Plain offers" }]) }) },
      preferences: { theme: "dark", language: "en" },
    });
    const { user } = renderApp();
    const pane = await openNewsletter(user);
    expect(await within(pane).findByText("Plain offers")).toBeTruthy();
    expect(within(pane).queryByRole("button", { name: "Original colours" })).toBeNull();
  });

  it("prints an adapted mail in its original colours", async () => {
    const core = setup("dark");
    const { user } = renderApp();
    const pane = await openNewsletter(user);
    await waitFor(() => {
      expect(frameIn(pane).getAttribute("data-colors")).toBe("adapted");
    });

    await user.click(screen.getByRole("button", { name: /^Print/ }));
    await waitFor(() => {
      expect(core.extras.state.prints).toBe(1);
    });
    expect(frameIn(pane).getAttribute("data-colors")).toBe("original");
    expect(core.callsOf("get_message").at(-1)).toEqual({ messageId: NEWSLETTER.id, format: "auto", scheme: "light" });
    // The light rendering was asked for before the print dialog opened.
    const order = core.calls.map((c) => c.cmd).filter((cmd) => cmd === "get_message" || cmd === "print_window");
    expect(order.at(-1)).toBe("print_window");
  });
});

describe("setting: messages in the dark theme", () => {
  it("is adapted by default and stores “always on a white page”", async () => {
    const core = mockCore({ accounts: [ACCOUNT], preferences: { theme: "dark", language: "en" } });
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: /^Settings/ }));
    await user.click(await screen.findByRole("tab", { name: "Reading" }));
    const block = await screen.findByTestId("setting-dark-mails");
    const adapted = within(block).getByRole("radio", { name: "Adapted to the dark theme" });
    expect(adapted.getAttribute("aria-checked")).toBe("true");

    await user.click(within(block).getByRole("radio", { name: "Always on a white page" }));
    await waitFor(() => {
      expect(core.callsOf("set_reading_preferences").at(-1)).toEqual({
        preferences: expect.objectContaining({ darkMails: "light" }) as unknown,
      });
    });
    expect(core.readingPreferences.darkMails).toBe("light");
  });
});
