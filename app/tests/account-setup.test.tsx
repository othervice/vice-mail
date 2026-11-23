import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { en } from "../src/i18n/en";
import { de } from "../src/i18n/de";
import type { DiscoveryResult, ErrorCode, ProviderHint } from "../src/services/ipc";
import { CAPABILITIES, ipcError, makeAccount, mockCore, type FakeCore } from "./support/core";
import { renderApp } from "./support/render";

/** Every variant of `ProviderHint`; the compile-time check below keeps the list complete. */
const PROVIDER_HINTS = [
  "gmail",
  "icloud",
  "yahoo",
  "fastmail",
  "outlook",
  "proton",
  "gmx",
  "webde",
  "tonline",
] as const satisfies readonly ProviderHint[];
const hintsAreComplete: [Exclude<ProviderHint, (typeof PROVIDER_HINTS)[number]>] extends [never] ? true : never = true;
void hintsAreComplete;

const GMAIL: DiscoveryResult = {
  source: "knownProvider",
  imap: { host: "imap.gmail.com", port: 993, security: "tls", username: "someone@gmail.com" },
  smtp: { host: "smtp.gmail.com", port: 465, security: "tls", username: "someone@gmail.com" },
  providerHint: "gmail",
};

const ICLOUD: DiscoveryResult = {
  source: "knownProvider",
  imap: { host: "imap.mail.me.com", port: 993, security: "tls", username: "someone@icloud.com" },
  smtp: { host: "smtp.mail.me.com", port: 587, security: "starttls", username: "someone@icloud.com" },
  providerHint: "icloud",
};

const AUTOCONFIG: DiscoveryResult = {
  source: "autoconfig",
  imap: { host: "mail.example.org", port: 993, security: "tls", username: "me@example.org" },
  smtp: { host: "mail.example.org", port: 587, security: "starttls", username: "me@example.org" },
  providerHint: null,
};

async function openSetup() {
  const view = renderApp();
  await view.user.click(await screen.findByRole("button", { name: "Add account" }));
  return view;
}

type User = Awaited<ReturnType<typeof openSetup>>["user"];

async function fillAddress(user: User, email: string, password = "app-password-123") {
  await user.type(screen.getByLabelText("Email address"), email);
  await user.type(screen.getByLabelText("Password"), password);
}

/** Fills the manual server form (shown when nothing is known about the domain). */
async function fillServers(user: User) {
  const imap = screen.getByRole("group", { name: "Incoming mail (IMAP)" });
  await user.type(within(imap).getByLabelText("Server"), "mail.vicemail.test");
  const smtp = screen.getByRole("group", { name: "Outgoing mail (SMTP)" });
  await user.type(within(smtp).getByLabelText("Server"), "mail.vicemail.test");
}

function networkCalls(core: FakeCore) {
  return core.callsOf("discover_online");
}

describe("account setup: known provider", () => {
  it("finds Gmail offline, warns about the app password and adds the account read-only", async () => {
    const core = mockCore({ knownProviders: { "gmail.com": GMAIL } });
    const { user } = await openSetup();

    await fillAddress(user, "someone@gmail.com");

    expect(await screen.findByText("Server settings for gmail.com are known.")).toBeTruthy();
    expect(screen.getByText("imap.gmail.com, port 993, TLS")).toBeTruthy();
    const hint = screen.getByTestId("provider-hint");
    expect(hint.textContent).toContain("Gmail needs an app password");
    // Stage 14: and that IMAP must be allowed (Google Workspace).
    expect(hint.textContent).toContain("Google Workspace account your administrator must allow IMAP");
    expect(hint.textContent).toContain("Your normal Google password does not work here.");
    expect(screen.getByTestId("read-only-note").textContent).toContain("changes nothing on the server");
    // Known provider: nothing is offered or fetched from the network.
    expect(screen.queryByRole("button", { name: "Search settings online" })).toBeNull();
    expect(networkCalls(core)).toEqual([]);
    expect(core.callsOf("discover_known_provider").at(-1)).toEqual({ email: "someone@gmail.com" });

    await user.click(screen.getByRole("button", { name: "Add account" }));

    await waitFor(() => {
      expect(core.callsOf("add_account")).toHaveLength(1);
    });
    expect(core.callsOf("add_account")[0]).toEqual({
      settings: {
        email: "someone@gmail.com",
        displayName: "",
        imap: GMAIL.imap,
        smtp: GMAIL.smtp,
      },
      password: "app-password-123",
    });
    expect(core.callsOf("test_connection")).toEqual([]);
    // Afterwards the inbox of the new account is shown.
    expect(await screen.findByRole("heading", { level: 1, name: "Inbox" })).toBeTruthy();
    expect(screen.getByRole("banner").textContent).toContain("someone@gmail.com");
  });

  it("shows the iCloud note about app-specific passwords", async () => {
    mockCore({ knownProviders: { "icloud.com": ICLOUD } });
    const { user } = await openSetup();

    await fillAddress(user, "someone@icloud.com");

    const hint = await screen.findByTestId("provider-hint");
    expect(hint.textContent).toContain("iCloud Mail needs an app-specific password");
    expect(hint.textContent).toContain("account.apple.com");
  });

  it.each(PROVIDER_HINTS)("has a note for the provider hint %s and shows it", async (hint) => {
    const texts = en.settings.accounts.setup.providerHint[hint];
    expect(texts.title.trim()).not.toBe("");
    expect(texts.body.trim()).not.toBe("");
    expect(de.settings.accounts.setup.providerHint[hint].title).not.toBe(texts.title);
    mockCore({ knownProviders: { "provider.test": { ...GMAIL, providerHint: hint } } });
    const { user } = await openSetup();

    await fillAddress(user, "someone@provider.test");

    const note = await screen.findByTestId("provider-hint");
    expect(note.textContent).toContain(texts.title);
    expect(note.textContent).toContain(texts.body);
  });

  it("says honestly that Outlook.com sign-in does not work yet", () => {
    expect(en.settings.accounts.setup.providerHint.outlook.title).toBe("Outlook.com accounts cannot be added yet");
    expect(en.settings.accounts.setup.providerHint.outlook.body).toMatch(/later version/);
  });

  it("no app-password note for other providers", async () => {
    mockCore();
    const { user } = await openSetup();
    await fillAddress(user, "ada@vicemail.test");
    await screen.findByRole("button", { name: "Search settings online" });
    expect(screen.queryByTestId("provider-hint")).toBeNull();
  });

  it("known settings can still be edited by hand", async () => {
    const core = mockCore({ knownProviders: { "gmail.com": GMAIL } });
    const { user } = await openSetup();
    await fillAddress(user, "someone@gmail.com");

    await user.click(await screen.findByRole("button", { name: "Edit server settings" }));
    const imap = screen.getByRole("group", { name: "Incoming mail (IMAP)" });
    expect(within(imap).getByLabelText<HTMLInputElement>("Server").value).toBe("imap.gmail.com");
    await user.clear(within(imap).getByLabelText("Port"));
    await user.type(within(imap).getByLabelText("Port"), "1993");
    await user.click(screen.getByRole("button", { name: "Add account" }));

    await waitFor(() => {
      expect(core.callsOf("add_account")).toHaveLength(1);
    });
    expect(core.callsOf("add_account")[0]).toMatchObject({ settings: { imap: { port: 1993 } } });
  });
});

describe("account setup: unknown domain", () => {
  it("contacts the network only after the click, explains where to, and uses what it finds", async () => {
    const core = mockCore({ online: { "example.org": AUTOCONFIG } });
    const { user } = await openSetup();
    await fillAddress(user, "me@example.org");

    const offer = await screen.findByTestId("online-search");
    expect(offer.textContent).toContain("The app does not know the servers of example.org.");
    expect(offer.textContent).toContain("autoconfig.example.org");
    expect(offer.textContent).toContain("only when you click");
    expect(networkCalls(core)).toEqual([]);

    await user.click(within(offer).getByRole("button", { name: "Search settings online" }));

    expect(networkCalls(core)).toEqual([{ email: "me@example.org" }]);
    // Online answers are not verified: the found servers are shown, with a note to check them,
    // before the user adds the account.
    const summary = await screen.findByTestId("server-summary");
    expect(summary.textContent).toContain("Server settings found at autoconfig.example.org.");
    expect(summary.textContent).toContain("Check that these servers belong to your provider.");
    expect(within(summary).getByText("mail.example.org, port 993, TLS")).toBeTruthy();
    expect(within(summary).getByText("mail.example.org, port 587, STARTTLS")).toBeTruthy();
    expect(within(summary).getAllByText("User name: me@example.org")).toHaveLength(2);
    expect(core.callsOf("add_account")).toEqual([]);

    await user.click(screen.getByRole("button", { name: "Add account" }));
    await waitFor(() => {
      expect(core.callsOf("add_account")).toHaveLength(1);
    });
    expect(core.callsOf("add_account")[0]).toMatchObject({ settings: { imap: AUTOCONFIG.imap, smtp: AUTOCONFIG.smtp } });
  });

  it("nothing found online: says so; the form by hand works, and the test reports the result", async () => {
    const core = mockCore({ capabilities: { ...CAPABILITIES, idle: false } });
    const { user } = await openSetup();
    await fillAddress(user, "ada@vicemail.test");

    await user.click(await screen.findByRole("button", { name: "Search settings online" }));
    expect(await screen.findByText("Nothing found. Enter the settings your provider gives you.")).toBeTruthy();

    await fillServers(user);
    const imap = screen.getByRole("group", { name: "Incoming mail (IMAP)" });
    // The user name follows the address until changed; STARTTLS switches the default port.
    expect(within(imap).getByLabelText<HTMLInputElement>("User name").value).toBe("ada@vicemail.test");
    await user.click(within(imap).getByRole("radio", { name: "STARTTLS" }));
    expect(within(imap).getByLabelText<HTMLInputElement>("Port").value).toBe("143");

    await user.click(screen.getByRole("button", { name: "Test connection" }));

    const result = await screen.findByTestId("test-result");
    expect(result.textContent).toContain("The connection works and the server accepted the login.");
    expect(result.textContent).toContain("new mail shows up with a delay");
    expect(core.callsOf("test_connection")).toEqual([
      {
        settings: {
          email: "ada@vicemail.test",
          displayName: "",
          imap: { host: "mail.vicemail.test", port: 143, security: "starttls", username: "ada@vicemail.test" },
          smtp: { host: "mail.vicemail.test", port: 465, security: "tls", username: "ada@vicemail.test" },
        },
        password: "app-password-123",
      },
    ]);
    // A test stores nothing.
    expect(core.callsOf("add_account")).toEqual([]);
  });

  it("does not submit an incomplete form and says what is missing", async () => {
    const core = mockCore();
    const { user } = await openSetup();

    await user.click(screen.getByRole("button", { name: "Add account" }));

    expect(screen.getByText("Enter a complete email address, for example name@example.com.")).toBeTruthy();
    expect(screen.getByLabelText("Password").getAttribute("aria-invalid")).toBe("true");
    expect(core.callsOf("add_account")).toEqual([]);
    expect(core.callsOf("test_connection")).toEqual([]);
  });
});

/** Every code `testConnection` and `addAccount` can answer with (IPC contract of stage 2). */
const SETUP_CODES: ErrorCode[] = [
  "invalid_argument",
  "dns_failed",
  "connection_refused",
  "connection_timeout",
  "connection_lost",
  "cert_untrusted",
  "cert_name_mismatch",
  "cert_expired",
  "tls_failed",
  "starttls_unavailable",
  "auth_failed",
  "unsupported_server",
  "protocol",
  "server_rejected",
  "account_exists",
  "keychain",
  "database",
];

describe("account setup: errors", () => {
  it.each(SETUP_CODES)("a failed test with %s says what is wrong and how to fix it", async (code) => {
    mockCore({ failures: { test_connection: ipcError(code, `detail for ${code}`) } });
    const { user } = await openSetup();
    await fillAddress(user, "ada@vicemail.test");
    await screen.findByRole("button", { name: "Search settings online" });
    await fillServers(user);

    await user.click(screen.getByRole("button", { name: "Test connection" }));

    const alert = await screen.findByTestId("setup-error");
    expect(alert.getAttribute("role")).toBe("alert");
    expect(alert.textContent).toContain("The connection test failed.");
    expect(alert.textContent).toContain(en.errors.codes[code].title);
    expect(alert.textContent).toContain(en.errors.codes[code].hint);
    expect(alert.textContent).toContain(`detail for ${code}`);
  });

  it("gives each connection problem its own advice", () => {
    const texts = SETUP_CODES.map((code) => en.errors.codes[code].title + en.errors.codes[code].hint);
    expect(new Set(texts).size).toBe(texts.length);
    // The three cases named in the stage: wrong password, untrusted certificate, unreachable.
    expect(en.errors.codes.auth_failed.title).toMatch(/password/);
    expect(en.errors.codes.cert_untrusted.title).toMatch(/not trusted/);
    expect(en.errors.codes.connection_refused.hint).toMatch(/server name and port/);
  });

  it("a rejected add shows the error and keeps the form, including the password", async () => {
    const core = mockCore({
      knownProviders: { "gmail.com": GMAIL },
      failures: { add_account: ipcError("auth_failed", "NO [AUTHENTICATIONFAILED]") },
    });
    const { user } = await openSetup();
    await fillAddress(user, "someone@gmail.com");
    await screen.findByText("Server settings for gmail.com are known.");

    await user.click(screen.getByRole("button", { name: "Add account" }));

    const alert = await screen.findByTestId("setup-error");
    expect(alert.textContent).toContain("The account could not be added.");
    expect(alert.textContent).toContain(en.errors.codes.auth_failed.title);
    // The app-password note stays next to it, which is the likely fix for Gmail.
    expect(screen.getByTestId("provider-hint")).toBeTruthy();
    expect(screen.getByLabelText<HTMLInputElement>("Password").value).toBe("app-password-123");
    expect(screen.getByRole("heading", { level: 2, name: "Add account" })).toBeTruthy();
    expect(core.accounts).toEqual([]);
  });

  it("an existing account is named as such", async () => {
    mockCore({
      accounts: [makeAccount({ email: "someone@gmail.com" })],
      knownProviders: { "gmail.com": GMAIL },
      failures: { add_account: ipcError("account_exists", "someone@gmail.com") },
    });
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Accounts" }));
    await user.click(await screen.findByRole("button", { name: "Add account" }));
    await fillAddress(user, "someone@gmail.com");
    await screen.findByText("Server settings for gmail.com are known.");

    await user.click(screen.getByRole("button", { name: "Add account" }));

    expect((await screen.findByTestId("setup-error")).textContent).toContain("This account is already set up.");
  });
});
