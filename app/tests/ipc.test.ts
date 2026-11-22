import { emit } from "@tauri-apps/api/event";
import { describe, expect, expectTypeOf, it } from "vitest";
import { CoreError, ipc, type AppInfo, type CoreEvent, type Preferences } from "../src/services/ipc";
import { APP_INFO, mockCore } from "./support/core";

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
