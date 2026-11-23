// Stage 14 (decision 0042): "Sign in with Microsoft/Google" in the account setup and "Sign in
// again" on the account's page. The browser step runs in the core; the UI shows its states.
import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { DiscoveryResult } from "../src/services/ipc";
import { oauthProviderFor } from "../src/features/settings/accounts/oauth";
import { en } from "../src/i18n/en";
import { de } from "../src/i18n/de";
import { IDLE, ipcError, makeAccount, mockCore } from "./support/core";
import { renderApp } from "./support/render";

const OUTLOOK: DiscoveryResult = {
  source: "knownProvider",
  imap: { host: "outlook.office365.com", port: 993, security: "tls", username: "ada@outlook.com" },
  smtp: { host: "smtp-mail.outlook.com", port: 587, security: "starttls", username: "ada@outlook.com" },
  providerHint: "outlook",
};

const GMAIL: DiscoveryResult = {
  source: "knownProvider",
  imap: { host: "imap.gmail.com", port: 993, security: "tls", username: "ada@gmail.com" },
  smtp: { host: "smtp.gmail.com", port: 465, security: "tls", username: "ada@gmail.com" },
  providerHint: "gmail",
};

async function openSetup() {
  const view = renderApp();
  await view.user.click(await screen.findByRole("button", { name: "Add account" }));
  return view;
}

describe("account setup: sign-in with Microsoft", () => {
  it("offers the sign-in instead of a password and adds the account read-only", async () => {
    const core = mockCore({ knownProviders: { "outlook.com": OUTLOOK }, oauth: { availability: { microsoft: true } } });
    const { user } = await openSetup();
    await user.type(screen.getByLabelText("Email address"), "ada@outlook.com");

    const box = await screen.findByTestId("oauth-setup");
    expect(box.textContent).toContain("never sees your Microsoft password");
    // Microsoft accepts no password: no password field, no "Add account" or "Test connection".
    expect(screen.queryByLabelText("Password")).toBeNull();
    expect(screen.queryByTestId("setup-submit")).toBeNull();
    expect(screen.queryByTestId("setup-test")).toBeNull();
    expect(screen.getByTestId("provider-hint").textContent).toContain("Outlook.com needs the sign-in with Microsoft");

    await user.click(within(box).getByRole("button", { name: "Sign in with Microsoft" }));
    await waitFor(() => {
      expect(core.oauth.added).toHaveLength(1);
    });
    const request = core.oauth.added[0];
    expect(request?.provider).toBe("microsoft");
    expect(request?.settings.email).toBe("ada@outlook.com");
    expect(request?.settings.imap.host).toBe("outlook.office365.com");
    // The browser page speaks the app's language.
    expect(request?.texts.done).toBe(en.settings.accounts.oauth.page.done);
    await waitFor(() => {
      expect(core.accounts.some((a) => a.oauth === "microsoft" && !a.writesEnabled)).toBe(true);
    });
  });

  it("shows the wait for the browser and lets the user cancel it", async () => {
    const core = mockCore({
      knownProviders: { "outlook.com": OUTLOOK },
      oauth: { availability: { microsoft: true }, outcome: "wait" },
    });
    const { user } = await openSetup();
    await user.type(screen.getByLabelText("Email address"), "ada@outlook.com");
    await user.click(await screen.findByRole("button", { name: "Sign in with Microsoft" }));

    const waiting = await screen.findByTestId("oauth-waiting");
    expect(waiting.textContent).toContain("Waiting for the sign-in with Microsoft");
    expect(waiting.textContent).toContain("five minutes");
    expect(screen.queryByRole("button", { name: "Sign in with Microsoft" })).toBeNull();

    await user.click(within(waiting).getByRole("button", { name: "Cancel sign-in" }));
    expect(await screen.findByTestId("oauth-cancelled")).toBeTruthy();
    expect(screen.queryByTestId("oauth-error")).toBeNull();
    expect(core.oauth.cancels).toBe(1);
    expect(core.accounts).toHaveLength(0);
    // It can start again.
    expect(screen.getByRole("button", { name: "Sign in with Microsoft" })).toBeTruthy();
  });

  it("shows why a sign-in failed", async () => {
    mockCore({
      knownProviders: { "outlook.com": OUTLOOK },
      oauth: { availability: { microsoft: true }, outcome: ipcError("oauth_denied", "access_denied: The user declined.") },
    });
    const { user } = await openSetup();
    await user.type(screen.getByLabelText("Email address"), "ada@outlook.com");
    await user.click(await screen.findByRole("button", { name: "Sign in with Microsoft" }));

    const error = await screen.findByTestId("oauth-error");
    expect(error.getAttribute("data-error-code")).toBe("oauth_denied");
    expect(error.textContent).toContain("The sign-in did not work.");
    expect(error.textContent).toContain(en.errors.codes.oauth_denied.title);
  });

  it("says so when this build cannot sign in with Microsoft", async () => {
    const core = mockCore({ knownProviders: { "outlook.com": OUTLOOK } });
    const { user } = await openSetup();
    await user.type(screen.getByLabelText("Email address"), "ada@outlook.com");

    const note = await screen.findByTestId("oauth-unavailable");
    expect(note.textContent).toContain("This version of the app cannot sign in with Microsoft");
    expect(screen.queryByRole("button", { name: "Sign in with Microsoft" })).toBeNull();
    expect(screen.queryByLabelText("Password")).toBeNull();
    expect(core.callsOf("add_oauth_account")).toHaveLength(0);
  });
});

describe("account setup: sign-in with Google", () => {
  it("is offered next to the app password when the build has it", async () => {
    const core = mockCore({ knownProviders: { "gmail.com": GMAIL }, oauth: { availability: { google: true } } });
    const { user } = await openSetup();
    await user.type(screen.getByLabelText("Email address"), "ada@gmail.com");

    const box = await screen.findByTestId("oauth-setup");
    expect(screen.getByLabelText("Password")).toBeTruthy();
    expect(screen.getByTestId("setup-submit")).toBeTruthy();
    await user.click(within(box).getByRole("button", { name: "Sign in with Google" }));
    await waitFor(() => {
      expect(core.oauth.added.map((r) => r.provider)).toEqual(["google"]);
    });
  });

  it("is hidden when the build has no Google client", async () => {
    mockCore({ knownProviders: { "gmail.com": GMAIL } });
    const { user } = await openSetup();
    await user.type(screen.getByLabelText("Email address"), "ada@gmail.com");
    expect(await screen.findByTestId("provider-hint")).toBeTruthy();
    expect(screen.queryByTestId("oauth-setup")).toBeNull();
    expect(screen.queryByTestId("oauth-unavailable")).toBeNull();
    expect(screen.getByLabelText("Password")).toBeTruthy();
  });

  it("recognises the providers by their servers too", () => {
    expect(oauthProviderFor(null, "outlook.office365.com")).toBe("microsoft");
    expect(oauthProviderFor(null, "Outlook.Office.com.")).toBe("microsoft");
    expect(oauthProviderFor(null, "imap.gmail.com")).toBe("google");
    expect(oauthProviderFor("outlook", "")).toBe("microsoft");
    expect(oauthProviderFor("gmail", "mail.example.org")).toBeNull();
    expect(oauthProviderFor(null, "outlook.office365.com.evil.example")).toBeNull();
  });
});

describe("account settings: an account that signs in with its provider", () => {
  const SIGNED_OUT = makeAccount({
    email: "ada@outlook.com",
    oauth: "microsoft",
    status: {
      state: "error",
      progress: null,
      error: ipcError("oauth_sign_in_required", "invalid_grant"),
      retryAt: null,
      lastSyncAt: 1,
    },
  });

  async function accountDetail(user: ReturnType<typeof renderApp>["user"], id: string) {
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Accounts" }));
    await user.click(await screen.findByTestId(`account-row-${id}`));
    return screen.findByTestId("account-detail");
  }

  it("asks to sign in again instead of for a password", async () => {
    const core = mockCore({ accounts: [SIGNED_OUT], oauth: { availability: { microsoft: true } } });
    const { user } = renderApp();
    const detail = await accountDetail(user, SIGNED_OUT.id);

    const block = await within(detail).findByTestId("oauth-account");
    expect(block.textContent).toContain("Signed in with Microsoft");
    expect(within(detail).queryByTestId("password-form")).toBeNull();
    expect(within(block).getByTestId("oauth-sign-in-required").textContent).toContain("expired or was withdrawn");

    await user.click(within(block).getByRole("button", { name: "Sign in again" }));
    await waitFor(() => {
      expect(core.oauth.again).toEqual([SIGNED_OUT.id]);
    });
    expect(await within(block).findByText("Signed in again. The account syncs again.")).toBeTruthy();
  });

  it("has no way to sign in again in a build without that provider", async () => {
    mockCore({ accounts: [{ ...SIGNED_OUT, status: IDLE }] });
    const { user } = renderApp();
    const detail = await accountDetail(user, SIGNED_OUT.id);
    const block = await within(detail).findByTestId("oauth-account");
    expect(within(block).getByTestId("oauth-unavailable")).toBeTruthy();
    expect(within(block).queryByRole("button", { name: "Sign in again" })).toBeNull();
    expect(within(detail).queryByTestId("password-form")).toBeNull();
  });
});

describe("texts", () => {
  it("exist in German with the informal address", () => {
    expect(de.settings.accounts.oauth.microsoft.button).toBe("Mit Microsoft anmelden");
    expect(de.settings.accounts.oauth.waitingBody).toContain("deinem Browser");
    expect(de.errors.codes.oauth_sign_in_required.hint).toContain("melde dich erneut an");
  });
});
