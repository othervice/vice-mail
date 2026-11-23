import { screen, waitFor, within } from "@testing-library/react";
import { mockWindows } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { parseWindowRoute } from "../src/features/windows/route";
import type { EmlMessageView, MessageViewHeader } from "../src/services/ipc";
import { htmlBody, ipcError, makeAccount, makeAttachment, mockCore, renderToken, standardMailboxes, textBody } from "./support/core";
import type { EmlFakeOptions } from "./support/eml";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";
import { makeComposeRequest } from "./support/extras";
import { useMailtoStore } from "../src/stores/mailto";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

const FILE = "0123456789abcdef0123456789abcdef";
const OTHER = "fedcba9876543210fedcba9876543210";
const TOKEN = renderToken();

const HEADER: MessageViewHeader = {
  from: [{ name: "Grace Hopper", email: "grace@example.org" }],
  to: [{ name: "Ada", email: "ada@example.org" }],
  cc: [],
  bcc: [],
  replyTo: [],
  date: new Date(2026, 10, 23, 8, 0).getTime(),
  subject: "Minutes from a file",
};

function emlView(overrides: Partial<EmlMessageView> = {}): EmlMessageView {
  return {
    fileId: FILE,
    fileName: "Minutes.eml",
    header: HEADER,
    body: htmlBody("<p>Hello from the file</p>", { renderToken: TOKEN, blockedRemote: 2 }),
    formats: ["html", "text"],
    attachments: [
      makeAttachment({ index: 0, name: "agenda.pdf", mimeType: "application/pdf" }),
      makeAttachment({ index: 1, name: "setup.exe", mimeType: "application/x-msdownload", executable: true }),
    ],
    problems: [],
    ...overrides,
  };
}

/** The page of an `.eml` window; the shell names it `message-file-<id>`. */
function openEmlWindow(fileId: string, eml: EmlFakeOptions, extra: Parameters<typeof mockCore>[0] = {}) {
  window.history.replaceState(null, "", `/?window=eml&fileId=${fileId}`);
  mockWindows(`message-file-${fileId}`);
  const core = mockCore({ accounts: [makeAccount()], mailboxes: standardMailboxes(), eml, ...extra });
  return { core, ...renderApp() };
}

describe(".eml files (decision 0027)", () => {
  it("routes ?window=eml with a core-made file id, and nothing else", () => {
    expect(parseWindowRoute(`?window=eml&fileId=${FILE}`)).toEqual({ kind: "eml", fileId: FILE });
    expect(parseWindowRoute(`?window=eml&fileId=${FILE.toUpperCase()}`)).toEqual({ kind: "invalid" });
    expect(parseWindowRoute("?window=eml&fileId=../../etc/passwd")).toEqual({ kind: "invalid" });
    expect(parseWindowRoute("?window=eml")).toEqual({ kind: "invalid" });
  });

  it("shows the file read-only through the reading pipeline: sandboxed frame, blocked content, file instead of account", async () => {
    const { core, user } = openEmlWindow(FILE, {
      files: { [FILE]: emlView() },
      remoteViews: { [FILE]: emlView({ body: htmlBody("<p>Hello</p>", { remoteLoaded: true }) }) },
    });
    const pane = await screen.findByTestId("eml-pane");
    expect(await within(pane).findByRole("heading", { name: "Minutes from a file" })).toBeTruthy();
    expect(within(pane).getByTestId("eml-read-only").textContent).toContain("It is not part of any account");
    expect(within(pane).getByTestId("reading-file").textContent).toBe("Minutes.eml");
    expect(within(pane).queryByTestId("reading-account")).toBeNull();
    expect(within(pane).getByTestId("reading-from").textContent).toContain("Grace Hopper");
    expect(within(pane).getByTestId("reading-date")).toBeTruthy();
    // The same frame as for stored mail: nothing in it runs.
    const frame = within(pane).getByTestId("mail-frame");
    expect(frame.getAttribute("sandbox")).toBe("allow-same-origin");
    expect(frame.getAttribute("srcdoc")).toContain("Hello from the file");
    // External content: only "load once" (no account, so no sender list), loaded by the core.
    const bar = within(pane).getByTestId("remote-content-bar");
    expect(within(bar).queryByTestId("remote-sender")).toBeNull();
    expect(core.callsOf("get_eml_message")).toEqual([{ fileId: FILE, format: "auto" }]);
    await user.click(within(bar).getByRole("button", { name: "Load once" }));
    await waitFor(() => {
      expect(within(pane).queryByTestId("remote-content-bar")).toBeNull();
    });
    expect(core.callsOf("load_eml_remote_content")).toEqual([{ fileId: FILE }]);
    // No mail actions: the message lies in no folder.
    expect(within(pane).queryByRole("toolbar")).toBeNull();
    // Nothing of an account's mail was asked for.
    expect(core.callsOf("get_message")).toEqual([]);
  });

  it("shows a text-only file without a format switch and without the date it does not have", async () => {
    const { core } = openEmlWindow(FILE, {
      files: { [FILE]: emlView({ header: { ...HEADER, date: 0 }, body: textBody([{ kind: "text", text: "Plain words" }]), formats: ["text"] }) },
    });
    const pane = await screen.findByTestId("eml-pane");
    expect(await within(pane).findByText("Plain words")).toBeTruthy();
    expect(within(pane).queryByTestId("reading-date")).toBeNull();
    expect(within(pane).queryByTestId("format-html")).toBeNull();
    expect(core.callsOf("get_eml_message")).toHaveLength(1);
  });

  it("saves attachments and opens a program only after the warning, all through the core", async () => {
    const { core, user } = openEmlWindow(FILE, { files: { [FILE]: emlView() } });
    const pane = await screen.findByTestId("eml-pane");
    await user.click(await within(pane).findByRole("button", { name: "Save agenda.pdf" }));
    expect(await within(pane).findByText("agenda.pdf saved to /Users/ada/Downloads/agenda.pdf")).toBeTruthy();
    expect(core.eml.saved).toEqual([`${FILE}/0`]);
    await user.click(within(pane).getByRole("button", { name: "Open setup.exe" }));
    const warning = await within(pane).findByTestId("attachment-warning");
    expect(core.eml.opened).toEqual([]);
    await user.click(within(warning).getByTestId("attachment-confirm"));
    await waitFor(() => {
      expect(core.eml.opened).toEqual([`${FILE}/1!`]);
    });
    expect(core.callsOf("save_attachment")).toEqual([]);
  });

  it("checks a clicked link in the core before anything opens", async () => {
    openEmlWindow(
      FILE,
      { files: { [FILE]: emlView() } },
      {
        links: {
          [`${TOKEN}/0`]: {
            target: "https://evil.example/login",
            scheme: "https",
            host: "evil.example",
            text: "https://bank.example",
            warnings: [{ kind: "domainMismatch", shownDomain: "bank.example", targetDomain: "evil.example" }],
            openable: true,
          },
        },
      },
    );
    const pane = await screen.findByTestId("eml-pane");
    await within(pane).findByTestId("mail-frame");
    await emitCore({ type: "linkActivated", renderToken: TOKEN, index: 0 });
    const card = await within(pane).findByTestId("link-card");
    expect(await within(card).findByTestId("link-target")).toBeTruthy();
    expect(within(card).getByTestId("link-host").textContent).toBe("evil.example");
    expect(within(card).getByTestId("link-warnings")).toBeTruthy();
  });

  it("writes to a mailto link of the file in this window, but leaves links from the system to the main window", async () => {
    const { core } = openEmlWindow(FILE, { files: { [FILE]: emlView() } });
    await screen.findByTestId("mail-frame");
    // A link from the system (composeRequested) reaches every window; only the main one takes it.
    await emitCore({ type: "composeRequested", request: makeComposeRequest({ to: [{ name: null, email: "lin@example.net" }] }) });
    expect(useMailtoStore.getState().pending).toEqual([]);
    expect(core.callsOf("create_draft")).toEqual([]);
    // A mailto link of the file, confirmed in the link card, becomes a draft here; nothing is sent.
    useMailtoStore.getState().enqueue([makeComposeRequest({ to: [{ name: null, email: "grace@example.org" }] })]);
    await waitFor(() => {
      expect(core.callsOf("create_draft")[0]?.["request"]).toMatchObject({ kind: "new", accountId: makeAccount().id });
    });
    expect(core.callsOf("send_draft")).toEqual([]);
  });

  it("says why a file the system handed over cannot be shown, and when it is no longer open", async () => {
    openEmlWindow(FILE, { files: { [FILE]: ipcError("eml_invalid", "no readable header") } });
    const alert = await screen.findByRole("alert");
    expect(alert.getAttribute("data-error-code")).toBe("eml_invalid");
    expect(within(alert).getByText("The file could not be shown.")).toBeTruthy();
    expect(within(alert).getByText("The file is not an email message.")).toBeTruthy();
  });

  it("says when the file is no longer open", async () => {
    openEmlWindow(OTHER, { files: {} });
    expect(await screen.findByRole("heading", { name: "This file is no longer open" })).toBeTruthy();
  });

  it("offers “Open .eml file…” in the palette; a refusal is a notice with the reason, cancelling says nothing", async () => {
    const core = mockCore({
      accounts: [makeAccount()],
      mailboxes: standardMailboxes(),
      eml: { dialog: { kind: "refuse", error: ipcError("eml_too_large", "70000000 bytes") } },
    });
    const { user } = renderApp();
    await screen.findByRole("navigation", { name: "Views" });
    await user.keyboard("{Meta>}k{/Meta}");
    const palette = await screen.findByRole("dialog", { name: "Command palette" });
    await waitFor(() => {
      expect(document.activeElement).toBe(within(palette).getByRole("combobox", { name: "Command" }));
    });
    await user.keyboard("eml file");
    await user.click(await within(palette).findByRole("option", { name: /Open \.eml file…/ }));
    await waitFor(() => {
      expect(core.eml.dialogs).toBe(1);
    });
    const notice = await screen.findByText("The file could not be opened.");
    expect(notice).toBeTruthy();
    expect(screen.getByText("The file is too large to open.")).toBeTruthy();
  });
});
