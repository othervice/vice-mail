import { mockIPC } from "@tauri-apps/api/mocks";
import type { AppInfo, IpcError, Preferences } from "../../src/services/ipc";

export const APP_INFO: AppInfo = {
  version: "0.1.0",
  commit: "1e235a5",
  buildDate: "2026-11-22T18:00:00Z",
  platform: "macos",
};

/** Rejection values a command can fail with: a typed IpcError or a framework string. */
export type Failure = IpcError | string;

interface FakeCoreOptions {
  appInfo?: AppInfo;
  preferences?: Preferences;
  failures?: Partial<Record<string, Failure>>;
  /** Simulate the event plugin (listen/emit). Off: event calls reach `failures` like commands. */
  mockEvents?: boolean;
}

export interface FakeCore {
  readonly calls: { cmd: string; args: unknown }[];
  readonly preferences: Preferences;
}

/**
 * Stands in for the Rust core behind `invoke`, with the command names and argument shapes of the
 * IPC contract. Events are mocked too, so `emit("core-event", …)` reaches `listen` handlers.
 */
export function mockCore(options: FakeCoreOptions = {}): FakeCore {
  const appInfo = options.appInfo ?? APP_INFO;
  let preferences: Preferences = options.preferences ?? { theme: "system", language: "system" };
  const calls: { cmd: string; args: unknown }[] = [];

  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      const failure = options.failures?.[cmd];
      // Tauri rejects with the serialised error value (an object or a plain string), not an Error.
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
      if (failure !== undefined) return Promise.reject(failure);
      switch (cmd) {
        case "get_app_info":
          return appInfo;
        case "get_preferences":
          return preferences;
        case "set_preferences":
          preferences = (args as { preferences: Preferences }).preferences;
          return preferences;
        default:
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          return Promise.reject(`command ${cmd} not found`);
      }
    },
    { shouldMockEvents: options.mockEvents ?? true },
  );

  return {
    calls,
    get preferences() {
      return preferences;
    },
  };
}
