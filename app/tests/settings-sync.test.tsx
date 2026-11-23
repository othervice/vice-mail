import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ipcError, makeAccount, mockCore } from "./support/core";
import { emitCore } from "./support/events";
import { makeDevice, syncStatus } from "./support/mcpSync";
import { renderApp } from "./support/render";

const ADA = makeAccount();
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "ada@work.test", displayName: "", color: 3 });
const PASSPHRASE = "correct horse battery staple";
const WINDOWS = makeDevice({ id: "device-win", name: "Windows PC", system: "Windows (x86_64)", current: false });

function active(overrides: Parameters<typeof syncStatus>[0] = {}) {
  return syncStatus({
    state: "active",
    accountId: ADA.id,
    folder: "Settings-Sync",
    lastSyncAt: Date.UTC(2026, 10, 22, 9, 30),
    devices: [makeDevice(), WINDOWS],
    ...overrides,
  });
}

async function openSync(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await user.click(screen.getByRole("tab", { name: "Settings sync" }));
  const panel = screen.getByRole("tabpanel", { name: "Settings sync" });
  await within(panel).findByText(/Keeps settings, signatures/);
  return panel;
}

describe("Settings, settings sync: turning it on", () => {
  it("turns on with a passphrase typed twice and says it writes only in the sync folder", async () => {
    const core = mockCore({ accounts: [ADA, WORK] });
    const { user } = renderApp();
    const panel = await openSync(user);

    const form = await within(panel).findByTestId("sync-enable-form");
    const accounts = within(form).getByRole("radiogroup", { name: "Sync account" });
    expect(within(accounts).getAllByRole("radio").map((radio) => radio.textContent)).toEqual([ADA.email, WORK.email]);
    expect(form.textContent).toContain("The app may write only in the folder “Settings-Sync” of this account");
    expect(form.textContent).toContain("Nobody can recover a forgotten passphrase");
    await user.click(within(accounts).getByRole("radio", { name: WORK.email }));

    const passphrase = within(form).getByLabelText("Passphrase");
    const repeat = within(form).getByLabelText("Repeat the passphrase");
    await user.type(passphrase, "short");
    await user.click(within(form).getByRole("button", { name: "Turn on settings sync" }));
    expect(within(form).getByText("At least 8 characters.")).toBeTruthy();
    expect(passphrase.getAttribute("aria-invalid")).toBe("true");

    await user.clear(passphrase);
    await user.type(passphrase, PASSPHRASE);
    await user.type(repeat, "correct horse battery");
    await user.click(within(form).getByRole("button", { name: "Turn on settings sync" }));
    expect(within(form).getByText("The two passphrases are not the same.")).toBeTruthy();
    expect(core.callsOf("enable_settings_sync")).toEqual([]);

    await user.type(repeat, " staple");
    await user.click(within(form).getByRole("button", { name: "Turn on settings sync" }));

    expect(await within(panel).findByTestId("sync-state")).toBeTruthy();
    expect(core.callsOf("enable_settings_sync")).toEqual([{ accountId: WORK.id, passphrase: PASSPHRASE }]);
    expect(within(panel).getByTestId("sync-state").textContent).toBe("On");
    expect(within(panel).getByTestId("sync-status").textContent).toContain(WORK.email);
    expect(within(panel).getByTestId("sync-status").textContent).toContain("Settings-Sync");
    expect(within(within(panel).getByTestId("sync-devices")).getByText("This computer")).toBeTruthy();
  });

  it("switches to joining when the account already holds a sync, and shows a wrong passphrase", async () => {
    const core = mockCore({
      accounts: [ADA],
      mcpSync: {
        sync: {
          folderAccounts: [ADA.id],
          passphrase: PASSPHRASE,
          preview: {
            kinds: [
              { kind: "signature", added: 2, changed: 1, removed: 0 },
              { kind: "snooze", added: 0, changed: 0, removed: 3 },
              { kind: "futureKind", added: 1, changed: 0, removed: 0 },
            ],
            newAccounts: ["grace@work.test"],
            uploads: 4,
            devices: [WINDOWS],
          },
        },
      },
    });
    const { user, queryClient } = renderApp();
    const panel = await openSync(user);

    const form = await within(panel).findByTestId("sync-enable-form");
    await user.type(within(form).getByLabelText("Passphrase"), PASSPHRASE);
    await user.type(within(form).getByLabelText("Repeat the passphrase"), PASSPHRASE);
    await user.click(within(form).getByRole("button", { name: "Turn on settings sync" }));

    const notice = await within(panel).findByTestId("sync-already-set-up");
    expect(notice.textContent).toContain("This account already holds a settings sync.");
    const join = within(panel).getByTestId("sync-join-form");
    expect(join.textContent).toContain(`${ADA.email} holds a settings sync in the folder “Settings-Sync”`);

    await user.type(within(join).getByLabelText("Passphrase"), "wrong passphrase");
    await user.click(within(join).getByRole("button", { name: "Show what changes" }));
    const alert = await within(join).findByRole("alert");
    expect(alert.dataset["errorCode"]).toBe("sync_wrong_passphrase");
    expect(alert.textContent).toContain("The passphrase is not correct.");
    expect(alert.textContent).toContain("Nothing was changed.");
    expect(core.callsOf("join_settings_sync")).toEqual([]);

    await user.clear(within(join).getByLabelText("Passphrase"));
    await user.type(within(join).getByLabelText("Passphrase"), PASSPHRASE);
    await user.click(within(join).getByRole("button", { name: "Show what changes" }));

    const preview = await within(panel).findByTestId("sync-join-preview");
    expect(within(preview).getByTestId("sync-preview-kind-signature").textContent).toBe("Signatures2 new, 1 changed");
    expect(within(preview).getByTestId("sync-preview-kind-snooze").textContent).toBe("Snoozed mail3 removed");
    expect(within(preview).getByTestId("sync-preview-kind-futureKind").textContent).toBe("futureKind1 new");
    expect(within(preview).getByTestId("sync-preview-accounts").textContent).toBe("grace@work.test");
    expect(preview.textContent).toContain("4 entries from this computer will be uploaded.");
    expect(preview.textContent).toContain("Windows PC (Windows (x86_64)), last seen");
    expect(core.callsOf("preview_settings_sync_join")).toEqual([
      { accountId: ADA.id, passphrase: "wrong passphrase" },
      { accountId: ADA.id, passphrase: PASSPHRASE },
    ]);

    await user.click(within(preview).getByRole("button", { name: "Take over and merge" }));
    await within(panel).findByTestId("sync-state");
    expect(core.callsOf("join_settings_sync")).toEqual([{}]);
    expect(within(panel).getByTestId("sync-state").textContent).toBe("On");
    // The passphrase does not linger in the mutation cache once the forms are gone.
    await waitFor(() => {
      expect(JSON.stringify(queryClient.getMutationCache().getAll().map((m) => m.state))).not.toContain(PASSPHRASE);
    });
  });

  it("offers to join when the core detected a sync folder, and asks for the passwords of taken-over accounts", async () => {
    const core = mockCore({
      accounts: [ADA],
      mcpSync: {
        sync: {
          status: syncStatus({ detectedAccounts: [ADA.id] }),
          passphrase: PASSPHRASE,
          joined: { accountsNeedingPassword: [WORK.id] },
        },
      },
    });
    const { user } = renderApp();
    const panel = await openSync(user);

    const join = await within(panel).findByTestId("sync-join-form");
    expect(join.textContent).toContain(`${ADA.email} holds a settings sync`);
    await user.click(within(join).getByRole("button", { name: "Cancel" }));
    const detected = within(panel).getByTestId("sync-detected");
    expect(detected.textContent).toContain(`${ADA.email} holds a settings sync. Join it with the passphrase`);
    await user.click(within(detected).getByRole("button", { name: "Join with the passphrase" }));

    await user.type(within(panel).getByLabelText("Passphrase"), PASSPHRASE);
    await user.click(within(panel).getByRole("button", { name: "Show what changes" }));
    const preview = await within(panel).findByTestId("sync-join-preview");
    expect(preview.textContent).toContain("Nothing changes here");
    // The account arrives with the join (the core announces it with accountsChanged).
    core.accounts.push(WORK);
    await user.click(within(preview).getByRole("button", { name: "Take over and merge" }));

    const passwords = await within(panel).findByTestId("sync-passwords");
    const field = await within(passwords).findByLabelText(`Password for ${WORK.email}`);
    await user.type(field, "hunter2");
    await user.click(within(passwords).getByRole("button", { name: "Save password" }));
    await waitFor(() => {
      expect(within(panel).queryByTestId("sync-passwords")).toBeNull();
    });
    expect(core.callsOf("update_password")).toEqual([{ accountId: WORK.id, password: "hunter2" }]);
  });

  it("points to joining from the accounts page once a sync folder is detected", async () => {
    const core = mockCore({ accounts: [ADA] });
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "Accounts" }));
    await screen.findByTestId(`account-row-${ADA.id}`);
    expect(screen.queryByTestId("sync-detected-hint")).toBeNull();

    // The mail sync of the new account saw the folder.
    const detected = syncStatus({ detectedAccounts: [ADA.id] });
    core.mcpSync.setSyncStatus(detected);
    await emitCore({ type: "settingsSyncChanged", status: detected });
    const hint = await screen.findByTestId("sync-detected-hint");
    expect(hint.textContent).toContain(`${ADA.email} holds a settings sync.`);
    await user.click(within(hint).getByRole("button", { name: "Open settings sync" }));
    const panel = screen.getByRole("tabpanel", { name: "Settings sync" });
    expect(await within(panel).findByTestId("sync-join-form")).toBeTruthy();
  });

  it("says that an account is needed first", async () => {
    mockCore({ accounts: [] });
    const { user } = renderApp();
    const panel = await openSync(user);
    expect(await within(panel).findByText(/Add an account first/)).toBeTruthy();
    expect(within(panel).queryByTestId("sync-enable-form")).toBeNull();
  });

  it("shows a failure to turn on with its code's title and hint", async () => {
    mockCore({ accounts: [ADA], failures: { enable_settings_sync: ipcError("keychain") } });
    const { user } = renderApp();
    const panel = await openSync(user);
    const form = await within(panel).findByTestId("sync-enable-form");
    await user.type(within(form).getByLabelText("Passphrase"), PASSPHRASE);
    await user.type(within(form).getByLabelText("Repeat the passphrase"), PASSPHRASE);
    await user.click(within(form).getByRole("button", { name: "Turn on settings sync" }));
    const alert = await within(form).findByRole("alert");
    expect(alert.textContent).toContain("The settings sync could not be turned on.");
    expect(alert.dataset["errorCode"]).toBe("keychain");
  });

  it("shows a status load failure", async () => {
    mockCore({ accounts: [ADA], failures: { get_settings_sync_status: ipcError("database") } });
    const { user } = renderApp();
    const panel = await openSync(user);
    expect((await within(panel).findByRole("alert")).textContent).toContain("The settings sync could not be loaded.");
  });
});

describe("Settings, settings sync: while it runs", () => {
  it("shows the status and the devices, and syncs now", async () => {
    const core = mockCore({ accounts: [ADA], mcpSync: { sync: { status: active({ pendingChanges: 2, rejectedEntries: 1 }) } } });
    const { user } = renderApp();
    const panel = await openSync(user);

    const status = await within(panel).findByTestId("sync-status");
    expect(within(status).getByTestId("sync-state").textContent).toBe("On");
    expect(status.textContent).toContain(ADA.email);
    expect(status.textContent).toContain("Changes not yet uploaded:2");
    expect(within(panel).getByTestId("sync-rejected").textContent).toContain("1 entry in the sync folder was ignored");
    const devices = within(panel).getByRole("list", { name: "Devices" });
    expect(within(devices).getAllByRole("listitem").map((item) => item.dataset["testid"])).toEqual([
      "sync-device-device-this",
      "sync-device-device-win",
    ]);
    expect(within(devices).getByText("This computer")).toBeTruthy();

    await user.click(within(status).getByRole("button", { name: "Sync now" }));
    await waitFor(() => {
      expect(core.callsOf("sync_settings_now")).toEqual([{}]);
    });
  });

  it("shows a failed pass with the error's title and hint, and that mail is not affected", async () => {
    const core = mockCore({
      accounts: [ADA],
      mcpSync: { sync: { status: active({ state: "error", lastError: ipcError("auth_failed", "LOGIN rejected") }) } },
    });
    const { user } = renderApp();
    const panel = await openSync(user);

    const alert = await within(panel).findByTestId("sync-last-error");
    expect(alert.textContent).toContain("Reading and sending mail are not affected.");
    expect(alert.textContent).toContain("The server did not accept the user name or password.");
    expect(within(panel).getByTestId("sync-state").textContent).toBe("The last sync failed");

    await user.click(within(alert).getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(within(panel).getByTestId("sync-state").textContent).toBe("On");
    });
    expect(core.callsOf("sync_settings_now")).toEqual([{}]);
    expect(within(panel).queryByTestId("sync-last-error")).toBeNull();
  });

  it("shows a failed sync now where it happened", async () => {
    mockCore({ accounts: [ADA], mcpSync: { sync: { status: active() } }, failures: { sync_settings_now: ipcError("sync_key_invalid") } });
    const { user } = renderApp();
    const panel = await openSync(user);
    await user.click(await within(panel).findByRole("button", { name: "Sync now" }));
    const alert = await within(panel).findByRole("alert");
    expect(alert.textContent).toContain("The settings could not be synced.");
    expect(alert.textContent).toContain("The settings sync data in the mailbox cannot be read.");
  });

  it("asks for the passphrase again after a reset elsewhere", async () => {
    const core = mockCore({
      accounts: [ADA],
      mcpSync: {
        sync: {
          status: active({ state: "needsPassphrase", lastError: ipcError("sync_key_changed") }),
          passphrase: "the new passphrase",
        },
      },
    });
    const { user } = renderApp();
    const panel = await openSync(user);

    const block = await within(panel).findByTestId("sync-needs-passphrase");
    expect(within(block).getByRole("alert").textContent).toContain("The settings sync was reset on another device.");
    expect(within(panel).queryByRole("button", { name: "Sync now" })).toBeNull();
    expect(within(panel).queryByTestId("sync-change")).toBeNull();

    await user.type(within(block).getByLabelText("Passphrase"), "the new passphrase");
    await user.click(within(block).getByRole("button", { name: "Show what changes" }));
    await user.click(await within(block).findByRole("button", { name: "Take over and merge" }));
    await waitFor(() => {
      expect(within(panel).getByTestId("sync-state").textContent).toBe("On");
    });
    expect(core.callsOf("preview_settings_sync_join")).toEqual([{ accountId: ADA.id, passphrase: "the new passphrase" }]);
  });

  it("keeps an account another device removed when asked to", async () => {
    const core = mockCore({
      accounts: [ADA, WORK],
      mcpSync: { sync: { status: active({ questions: [{ type: "accountRemoved", accountId: WORK.id, email: WORK.email }] }) } },
    });
    const { user } = renderApp();
    const panel = await openSync(user);

    const question = await within(panel).findByTestId(`sync-question-${WORK.id}`);
    expect(question.textContent).toContain(`${WORK.email} was removed on another device.`);
    expect(question.textContent).toContain("Keeping it brings the account back on every device.");
    await user.click(within(question).getByRole("button", { name: "Keep" }));
    await waitFor(() => {
      expect(within(panel).queryByTestId("sync-questions")).toBeNull();
    });
    expect(core.callsOf("resolve_sync_question")).toEqual([{ accountId: WORK.id, remove: false }]);
    expect(core.accounts.map((account) => account.id)).toEqual([ADA.id, WORK.id]);
  });

  it("removes the account here too only after a second, deliberate click", async () => {
    const core = mockCore({
      accounts: [ADA, WORK],
      mcpSync: { sync: { status: active({ questions: [{ type: "accountRemoved", accountId: WORK.id, email: WORK.email }] }) } },
    });
    const { user } = renderApp();
    const panel = await openSync(user);

    const question = await within(panel).findByTestId(`sync-question-${WORK.id}`);
    await user.click(within(question).getByRole("button", { name: "Remove here too" }));
    const confirm = within(question).getByRole("group", { name: `Remove ${WORK.email} from this computer?` });
    expect(document.activeElement).toBe(confirm);
    expect(core.callsOf("resolve_sync_question")).toEqual([]);
    await user.click(within(confirm).getByRole("button", { name: "Cancel" }));
    await user.click(within(question).getByRole("button", { name: "Remove here too" }));
    await user.click(within(question).getByRole("button", { name: "Remove here too" }));

    await waitFor(() => {
      expect(within(panel).queryByTestId("sync-questions")).toBeNull();
    });
    expect(core.callsOf("resolve_sync_question")).toEqual([{ accountId: WORK.id, remove: true }]);
    expect(core.accounts.map((account) => account.id)).toEqual([ADA.id]);
  });

  it("renames this device", async () => {
    const core = mockCore({ accounts: [ADA], mcpSync: { sync: { status: active() } } });
    const { user } = renderApp();
    const panel = await openSync(user);

    const form = await within(panel).findByTestId("sync-device-name");
    const field = within(form).getByLabelText("Name of this computer");
    expect((field as HTMLInputElement).value).toBe("Mac");
    await user.clear(field);
    await user.click(within(form).getByRole("button", { name: "Save name" }));
    expect(within(form).getByText("Enter a name.")).toBeTruthy();
    await user.type(field, "Studio Mac");
    await user.click(within(form).getByRole("button", { name: "Save name" }));
    await waitFor(() => {
      expect(within(panel).getByTestId("sync-device-device-this").textContent).toContain("Studio Mac");
    });
    expect(core.callsOf("set_sync_device_name")).toEqual([{ name: "Studio Mac" }]);
  });

  it("changes the passphrase", async () => {
    const core = mockCore({ accounts: [ADA], mcpSync: { sync: { status: active(), passphrase: PASSPHRASE } } });
    const { user } = renderApp();
    const panel = await openSync(user);

    const block = await within(panel).findByTestId("sync-change");
    await user.type(within(block).getByLabelText("New passphrase"), "1234567");
    await user.click(within(block).getByRole("button", { name: "Change passphrase" }));
    expect(within(block).getByText("At least 8 characters.")).toBeTruthy();
    await user.clear(within(block).getByLabelText("New passphrase"));
    await user.type(within(block).getByLabelText("New passphrase"), "a much longer passphrase");
    await user.type(within(block).getByLabelText("Repeat the passphrase"), "a much longer passphrase");
    await user.click(within(block).getByRole("button", { name: "Change passphrase" }));
    expect(await within(block).findByText("Passphrase changed.")).toBeTruthy();
    expect(core.callsOf("change_settings_sync_passphrase")).toEqual([{ passphrase: "a much longer passphrase" }]);
    expect(core.mcpSync.syncPassphrase).toBe("a much longer passphrase");
    expect(within(block).getByLabelText<HTMLInputElement>("New passphrase").value).toBe("");
  });

  it("resets the sync after a confirmation", async () => {
    const core = mockCore({ accounts: [ADA], mcpSync: { sync: { status: active({ rejectedEntries: 3 }) } } });
    const { user } = renderApp();
    const panel = await openSync(user);

    const block = await within(panel).findByTestId("sync-reset");
    await user.type(within(block).getByLabelText("New passphrase"), "forgotten no more");
    await user.type(within(block).getByLabelText("Repeat the passphrase"), "forgotten no more");
    await user.click(within(block).getByRole("button", { name: "Reset with this passphrase" }));
    const confirm = within(block).getByRole("group", { name: "Reset the settings sync?" });
    expect(confirm.textContent).toContain("The other devices pause until you enter the new passphrase there.");
    expect(core.callsOf("reset_settings_sync")).toEqual([]);
    await user.click(within(confirm).getByRole("button", { name: "Reset the sync" }));

    expect(await within(block).findByText(/The sync was reset/)).toBeTruthy();
    expect(core.callsOf("reset_settings_sync")).toEqual([{ passphrase: "forgotten no more" }]);
    expect(within(panel).queryByTestId("sync-rejected")).toBeNull();
  });

  it("turns off and then offers to join the folder again", async () => {
    const core = mockCore({ accounts: [ADA], mcpSync: { sync: { status: active() } } });
    const { user } = renderApp();
    const panel = await openSync(user);

    await user.click(await within(panel).findByRole("button", { name: "Turn off settings sync" }));
    expect(await within(panel).findByTestId("sync-join-form")).toBeTruthy();
    expect(core.callsOf("disable_settings_sync")).toEqual([{}]);
    expect(within(panel).queryByTestId("sync-status")).toBeNull();
  });

  it("follows settingsSyncChanged", async () => {
    const core = mockCore({ accounts: [ADA], mcpSync: { sync: { status: active() } } });
    const { user } = renderApp();
    const panel = await openSync(user);
    await within(panel).findByTestId("sync-status");

    const next = active({
      state: "error",
      lastError: ipcError("connection_refused"),
      questions: [{ type: "accountRemoved", accountId: ADA.id, email: ADA.email }],
    });
    core.mcpSync.setSyncStatus(next);
    await emitCore({ type: "settingsSyncChanged", status: next });
    expect(await within(panel).findByTestId("sync-last-error")).toBeTruthy();
    expect(within(panel).getByTestId(`sync-question-${ADA.id}`)).toBeTruthy();
  });
});
