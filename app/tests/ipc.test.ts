import { emit } from "@tauri-apps/api/event";
import { describe, expect, expectTypeOf, it } from "vitest";
import { de } from "../src/i18n/de";
import { en } from "../src/i18n/en";
import {
  CoreError,
  ERROR_CODES,
  ipc,
  isIpcError,
  type AccountSettings,
  type AppInfo,
  type CoreEvent,
  type MessagePage,
  type Preferences,
} from "../src/services/ipc";
import { APP_INFO, makeAccount, makeMessages, mockCore, standardMailboxes } from "./support/core";

describe("commands", () => {
  it("getAppInfo returns the typed answer of the core", async () => {
    const core = mockCore();

    const info = await ipc.getAppInfo();

    expectTypeOf(info).toEqualTypeOf<AppInfo>();
    expect(info).toEqual(APP_INFO);
    expect(core.calls.map((c) => c.cmd)).toEqual(["get_app_info"]);
  });

  it("setPreferences sends the contract argument and returns what the core stored", async () => {
    const core = mockCore();
    const next: Preferences = { theme: "dark", language: "de" };

    const saved = await ipc.setPreferences(next);

    expectTypeOf(saved).toEqualTypeOf<Preferences>();
    expect(saved).toEqual(next);
    expect(core.calls).toEqual([{ cmd: "set_preferences", args: { preferences: next } }]);
    await expect(ipc.getPreferences()).resolves.toEqual(next);
  });
});

describe("stage 2 commands", () => {
  const settings: AccountSettings = {
    email: "ada@vicemail.test",
    displayName: "Ada",
    imap: { host: "localhost", port: 1993, security: "tls", username: "ada@vicemail.test" },
    smtp: { host: "localhost", port: 1465, security: "tls", username: "ada@vicemail.test" },
  };

  it("send the command names and argument names of the contract", async () => {
    const account = makeAccount();
    const core = mockCore({ accounts: [account], mailboxes: standardMailboxes(), messages: { 1: makeMessages(3) } });

    await ipc.listAccounts();
    await ipc.discoverKnownProvider("ada@vicemail.test");
    await ipc.discoverOnline("ada@vicemail.test");
    await ipc.testConnection(settings, "pw");
    await ipc.retryAccount(account.id);
    await ipc.updatePassword(account.id, "pw2");
    await ipc.listMailboxes(account.id);
    await ipc.setMailboxRole(5, "archive");
    await ipc.listMessages(1, null, 2);
    await ipc.addAccount(settings, "pw");
    await ipc.removeAccount(account.id);

    expect(core.calls).toEqual([
      { cmd: "list_accounts", args: {} },
      { cmd: "discover_known_provider", args: { email: "ada@vicemail.test" } },
      { cmd: "discover_online", args: { email: "ada@vicemail.test" } },
      { cmd: "test_connection", args: { settings, password: "pw" } },
      { cmd: "retry_account", args: { accountId: account.id } },
      { cmd: "update_password", args: { accountId: account.id, password: "pw2" } },
      { cmd: "list_mailboxes", args: { accountId: account.id } },
      { cmd: "set_mailbox_role", args: { mailboxId: 5, role: "archive" } },
      { cmd: "list_messages", args: { mailboxId: 1, cursor: null, limit: 2 } },
      { cmd: "add_account", args: { settings, password: "pw" } },
      { cmd: "remove_account", args: { accountId: account.id } },
    ]);
  });

  it("listMessages pages with the opaque cursor the core returns", async () => {
    mockCore({ messages: { 1: makeMessages(5) } });

    const first = await ipc.listMessages(1, null, 2);
    expectTypeOf(first).toEqualTypeOf<MessagePage>();
    expect(first.items.map((m) => m.subject)).toEqual(["Message 1", "Message 2"]);
    const second = await ipc.listMessages(1, first.nextCursor, 2);
    expect(second.items.map((m) => m.subject)).toEqual(["Message 3", "Message 4"]);
    const last = await ipc.listMessages(1, second.nextCursor, 2);
    expect(last.items.map((m) => m.subject)).toEqual(["Message 5"]);
    expect(last.nextCursor).toBeNull();
  });

  it("commands without a result resolve to undefined", async () => {
    mockCore({ accounts: [makeAccount()] });
    await expect(ipc.retryAccount(makeAccount().id)).resolves.toBeUndefined();
  });
});

describe("error codes", () => {
  /** The 21 codes of the stage 2 contract, in the order of `ErrorCode` in Rust. */
  const CONTRACT = [
    "internal",
    "invalid_argument",
    "not_found",
    "database",
    "database_too_new",
    "auth_failed",
    "cert_untrusted",
    "cert_name_mismatch",
    "cert_expired",
    "tls_failed",
    "starttls_unavailable",
    "connection_refused",
    "connection_timeout",
    "connection_lost",
    "dns_failed",
    "protocol",
    "server_rejected",
    "read_only",
    "unsupported_server",
    "keychain",
    "account_exists",
  ];

  it("ERROR_CODES holds exactly the codes of the contract", () => {
    expect([...ERROR_CODES]).toEqual(CONTRACT);
  });

  it.each(CONTRACT)("%s has a title and a way forward in English and German", (code) => {
    const key = code as (typeof ERROR_CODES)[number];
    for (const texts of [en.errors.codes[key], de.errors.codes[key]]) {
      expect(texts.title.trim()).not.toBe("");
      expect(texts.hint.trim()).not.toBe("");
    }
    expect(de.errors.codes[key].title).not.toBe(en.errors.codes[key].title);
    expect(isIpcError({ code, message: "m", detail: null })).toBe(true);
  });

  it("an unknown code is not taken for a typed error", () => {
    expect(isIpcError({ code: "no_such_code", message: "m", detail: null })).toBe(false);
  });
});

describe("errors", () => {
  it("a typed core error arrives as CoreError with its code, message and detail", async () => {
    mockCore({
      failures: {
        get_preferences: {
          code: "database_too_new",
          message: "The database was written by a newer version.",
          detail: "user_version 7 > 3",
        },
      },
    });

    const error = await ipc.getPreferences().catch((e: unknown) => e);

    expect(error).toBeInstanceOf(CoreError);
    expect(error).toMatchObject({
      code: "database_too_new",
      message: "The database was written by a newer version.",
      detail: "user_version 7 > 3",
    });
  });

  it("a framework error string (bad argument, unknown command, ACL) becomes code internal", async () => {
    const raw = "invalid args `preferences` for command `set_preferences`: missing field `theme`";
    mockCore({ failures: { set_preferences: raw } });

    const error = await ipc.setPreferences({ theme: "light", language: "en" }).catch((e: unknown) => e);

    expect(error).toBeInstanceOf(CoreError);
    expect(error).toMatchObject({ code: "internal", detail: raw });
  });

  it("a missing Tauri runtime also becomes code internal instead of a raw TypeError", async () => {
    // No mockCore(): window.__TAURI_INTERNALS__ does not exist, as in a plain browser.
    const error = await ipc.getAppInfo().catch((e: unknown) => e);

    expect(error).toBeInstanceOf(CoreError);
    expect(error).toMatchObject({ code: "internal" });
    expect((error as CoreError).detail).toBeTruthy();
  });
});

describe("event stream", () => {
  it("delivers typed core events until unsubscribed", async () => {
    mockCore();
    const received: CoreEvent[] = [];
    const event: CoreEvent = { type: "preferencesChanged", preferences: { theme: "dark", language: "de" } };

    const unsubscribe = await ipc.onCoreEvent((e) => {
      expectTypeOf(e).toEqualTypeOf<CoreEvent>();
      received.push(e);
    });
    await emit("core-event", event);
    unsubscribe();
    await emit("core-event", event);

    expect(received).toEqual([event]);
  });
});
