import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { DiscoveryResult } from "../src/services/ipc";
import { CAPABILITIES, makeAccount, mockCore } from "./support/core";
import { renderApp } from "./support/render";

// Stage 14, block D (decision 0041): JMAP as a protocol choice in the account setup.

const JMAP_FOUND: DiscoveryResult = {
  source: "autoconfig",
  imap: { host: "imap.example.org", port: 993, security: "tls", username: "me@example.org" },
  smtp: { host: "smtp.example.org", port: 465, security: "tls", username: "me@example.org" },
  providerHint: null,
  jmap: { host: "api.example.org", port: 443, security: "tls", username: "me@example.org" },
};

const JMAP_ONLY: DiscoveryResult = {
  source: "none",
  imap: null,
  smtp: null,
  providerHint: null,
  jmap: { host: "jmap.example.net", port: 8443, security: "tls", username: "me@example.net" },
};

async function openSetup() {
  const view = renderApp();
  await view.user.click(await screen.findByRole("button", { name: "Add account" }));
  return view;
}

type User = Awaited<ReturnType<typeof openSetup>>["user"];

async function fillAddress(user: User, email: string) {
  await user.type(screen.getByLabelText("Email address"), email);
  await user.type(screen.getByLabelText("Password"), "secret-pw");
}

describe("account setup: JMAP", () => {
  it("keeps IMAP as the default and adds a JMAP account with one server when chosen", async () => {
    const core = mockCore({ capabilities: { ...CAPABILITIES, imap4rev1: false, jmap: true, idle: true } });
    const { user } = await openSetup();
    await fillAddress(user, "bob@vicemail.test");

    const choice = await screen.findByRole("radiogroup", { name: "Protocol" });
    expect(within(choice).getByRole("radio", { name: "IMAP and SMTP" }).getAttribute("aria-checked")).toBe("true");
    expect(screen.getByRole("group", { name: "Incoming mail (IMAP)" })).toBeTruthy();
    expect(screen.queryByRole("group", { name: "JMAP server" })).toBeNull();

    await user.click(within(choice).getByRole("radio", { name: "JMAP" }));
    const server = screen.getByRole("group", { name: "JMAP server" });
    expect(screen.queryByRole("group", { name: "Incoming mail (IMAP)" })).toBeNull();
    expect(screen.queryByRole("group", { name: "Outgoing mail (SMTP)" })).toBeNull();
    expect(within(server).getByLabelText("User name")).toHaveProperty("value", "bob@vicemail.test");
    expect(within(server).getByLabelText("Port")).toHaveProperty("value", "443");
    // No security choice: JMAP is always HTTPS.
    expect(within(server).queryByRole("radiogroup", { name: "Security" })).toBeNull();
    await user.type(within(server).getByLabelText("Server"), "localhost");
    await user.clear(within(server).getByLabelText("Port"));
    await user.type(within(server).getByLabelText("Port"), "18443");

    await user.click(screen.getByRole("button", { name: "Test connection" }));
    expect((await screen.findByTestId("test-result")).textContent).toContain("the server pushes changes");
    const jmapServer = { host: "localhost", port: 18443, security: "tls", username: "bob@vicemail.test" };
    expect(core.callsOf("test_connection")[0]).toEqual({
      settings: { email: "bob@vicemail.test", displayName: "", imap: jmapServer, smtp: jmapServer, protocol: "jmap" },
      password: "secret-pw",
    });

    await user.click(screen.getByRole("button", { name: "Add account" }));
    await waitFor(() => {
      expect(core.callsOf("add_account")).toHaveLength(1);
    });
    expect(core.callsOf("add_account")[0]).toEqual({
      settings: { email: "bob@vicemail.test", displayName: "", imap: jmapServer, smtp: jmapServer, protocol: "jmap" },
      password: "secret-pw",
    });
    expect(await screen.findByRole("heading", { level: 1, name: "Inbox" })).toBeTruthy();
  });

  it("says that the domain offers JMAP too and fills it in when chosen", async () => {
    mockCore({ online: { "example.org": JMAP_FOUND } });
    const { user } = await openSetup();
    await fillAddress(user, "me@example.org");
    await user.click(await screen.findByRole("button", { name: "Search settings online" }));

    expect((await screen.findByTestId("jmap-found")).textContent).toBe(
      "example.org also offers JMAP (api.example.org). You can choose it under the server settings.",
    );
    await user.click(screen.getByRole("button", { name: "Edit server settings" }));
    const choice = screen.getByRole("radiogroup", { name: "Protocol" });
    await user.click(within(choice).getByRole("radio", { name: "JMAP" }));
    const server = screen.getByRole("group", { name: "JMAP server" });
    expect(within(server).getByLabelText("Server")).toHaveProperty("value", "api.example.org");
    expect(screen.queryByTestId("jmap-found")).toBeNull();
  });

  it("offers the form when only a JMAP server was found", async () => {
    mockCore({ online: { "example.net": JMAP_ONLY } });
    const { user } = await openSetup();
    await fillAddress(user, "me@example.net");
    await user.click(await screen.findByRole("button", { name: "Search settings online" }));

    expect(await screen.findByTestId("jmap-found")).toBeTruthy();
    expect(screen.queryByText("Nothing found. Enter the settings your provider gives you.")).toBeNull();
    const choice = screen.getByRole("radiogroup", { name: "Protocol" });
    await user.click(within(choice).getByRole("radio", { name: "JMAP" }));
    const server = screen.getByRole("group", { name: "JMAP server" });
    expect(within(server).getByLabelText("Port")).toHaveProperty("value", "8443");
  });

  it("shows one server on the account page of a JMAP account", async () => {
    const jmapServer = { host: "jmap.example.org", port: 443, security: "tls" as const, username: "me@example.org" };
    const account = makeAccount({ email: "me@example.org", imap: jmapServer, smtp: jmapServer, protocol: "jmap" });
    mockCore({ accounts: [account] });
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Accounts" }));
    await user.click(await screen.findByTestId(`account-row-${account.id}`));
    expect(await screen.findByText("JMAP server")).toBeTruthy();
    expect(screen.getByText("jmap.example.org, port 443, TLS")).toBeTruthy();
    expect(screen.queryByText("Incoming mail (IMAP)")).toBeNull();
  });

  // Stage 14: Fastmail's JMAP wants an API token (sent as a bearer token) instead of the password.
  it("signs in with an API token instead of the password when chosen", async () => {
    const core = mockCore({ capabilities: { ...CAPABILITIES, imap4rev1: false, jmap: true, idle: true } });
    const { user } = await openSetup();
    await user.type(screen.getByLabelText("Email address"), "me@fastmail.example");
    const choice = await screen.findByRole("radiogroup", { name: "Protocol" });
    // IMAP has no token choice.
    expect(screen.queryByRole("radiogroup", { name: "Sign in with" })).toBeNull();
    await user.click(within(choice).getByRole("radio", { name: "JMAP" }));
    const server = screen.getByRole("group", { name: "JMAP server" });
    const signIn = within(server).getByRole("radiogroup", { name: "Sign in with" });
    expect(within(signIn).getByRole("radio", { name: "Password" }).getAttribute("aria-checked")).toBe("true");
    expect(within(server).queryByLabelText("API token")).toBeNull();
    expect(screen.getByLabelText("Password")).toBeTruthy();

    await user.click(within(signIn).getByRole("radio", { name: "API token" }));
    // The token replaces the password field; the hint says where to create one.
    expect(screen.queryByLabelText("Password")).toBeNull();
    const field = within(server).getByLabelText("API token");
    expect(field.getAttribute("type")).toBe("password");
    expect(server.textContent).toContain(
      "At Fastmail: Settings → Privacy & Security → Manage API tokens → New API token, with access to “Email” and, for sending, “Email submission”.",
    );
    await user.type(within(server).getByLabelText("Server"), "api.fastmail.example");

    // Nothing is sent without a token.
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    expect(core.callsOf("test_connection")).toEqual([]);
    expect(within(server).getByText("Required.")).toBeTruthy();

    await user.type(field, "fmu1-secret-token");
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    await screen.findByTestId("test-result");
    const jmapServer = { host: "api.fastmail.example", port: 443, security: "tls", username: "me@fastmail.example" };
    const settings = {
      email: "me@fastmail.example",
      displayName: "",
      imap: jmapServer,
      smtp: jmapServer,
      protocol: "jmap",
      credential: "apiToken",
    };
    expect(core.callsOf("test_connection")).toEqual([{ settings, password: "fmu1-secret-token" }]);
    await user.click(screen.getByRole("button", { name: "Add account" }));
    await waitFor(() => {
      expect(core.callsOf("add_account")).toEqual([{ settings, password: "fmu1-secret-token" }]);
    });
  });

  it("goes back to the password field and leaves the token out", async () => {
    const core = mockCore({ capabilities: { ...CAPABILITIES, imap4rev1: false, jmap: true, idle: true } });
    const { user } = await openSetup();
    await fillAddress(user, "me@fastmail.example");
    await user.click(within(await screen.findByRole("radiogroup", { name: "Protocol" })).getByRole("radio", { name: "JMAP" }));
    const server = screen.getByRole("group", { name: "JMAP server" });
    const signIn = within(server).getByRole("radiogroup", { name: "Sign in with" });
    await user.click(within(signIn).getByRole("radio", { name: "API token" }));
    await user.type(within(server).getByLabelText("API token"), "fmu1-secret-token");
    await user.click(within(signIn).getByRole("radio", { name: "Password" }));
    await user.type(within(server).getByLabelText("Server"), "jmap.example.org");
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    await screen.findByTestId("test-result");
    const call = core.callsOf("test_connection")[0] as { settings: Record<string, unknown>; password: string };
    expect(call.password).toBe("secret-pw");
    expect(call.settings).not.toHaveProperty("credential");
  });

  it("replaces the API token of an account that signs in with one", async () => {
    const jmapServer = { host: "api.fastmail.example", port: 443, security: "tls" as const, username: "me@fastmail.example" };
    const account = makeAccount({
      email: "me@fastmail.example",
      imap: jmapServer,
      smtp: jmapServer,
      protocol: "jmap",
      credential: "apiToken",
    });
    const core = mockCore({ accounts: [account] });
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Accounts" }));
    await user.click(await screen.findByTestId(`account-row-${account.id}`));
    const form = await screen.findByTestId("password-form");
    expect(screen.getByText("This account signs in with an API token.", { exact: false })).toBeTruthy();
    await user.type(within(form).getByLabelText("New API token"), "fmu1-new-token");
    await user.click(within(form).getByRole("button", { name: "Save API token" }));
    expect(await screen.findByText("API token saved. The app reconnects with it.")).toBeTruthy();
    expect(core.callsOf("update_password")).toEqual([{ accountId: account.id, password: "fmu1-new-token" }]);
  });
});
