// The only module that talks to the core. Variant: tauri-specta (chosen).
// Command names, argument names, result/error types and the event name all come from
// the generated bindings, so a rename in Rust breaks the TypeScript build.
import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  commands,
  events,
  type CoreEvent,
  type ErrorCode,
  type IpcError,
  type MessageDetail,
  type MessageQuery,
} from "../generated/tauri-specta/bindings";

export type { CoreEvent, ErrorCode, IpcError, MessageDetail, MessageQuery };

export type Result<T> = { ok: true; value: T } | { ok: false; error: IpcError };

const ERROR_CODES: readonly string[] = [
  "not_found",
  "invalid_argument",
  "read_only",
  "internal",
] satisfies readonly ErrorCode[];

export function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    typeof v["code"] === "string" &&
    ERROR_CODES.includes(v["code"]) &&
    typeof v["message"] === "string" &&
    (v["detail"] === null || typeof v["detail"] === "string")
  );
}

/** Framework failures (unknown command, bad argument, ACL denial) reject with a plain string. */
export function toIpcError(value: unknown): IpcError {
  if (isIpcError(value)) return value;
  const detail = value instanceof Error ? value.message : String(value);
  return { code: "internal", message: "The app core rejected the request.", detail };
}

type SpectaResult<T> = { status: "ok"; data: T } | { status: "error"; error: IpcError };

async function call<T>(pending: Promise<SpectaResult<T>>): Promise<Result<T>> {
  try {
    const r = await pending;
    return r.status === "ok" ? { ok: true, value: r.data } : { ok: false, error: toIpcError(r.error) };
  } catch (e) {
    return { ok: false, error: toIpcError(e) };
  }
}

export const ipc = {
  getMessage: (query: MessageQuery): Promise<Result<MessageDetail>> => call(commands.getMessage(query)),
  startDemoSync: (accountId: string): Promise<Result<null>> => call(commands.startDemoSync(accountId)),
  reportFrontend: (ok: boolean, summary: string): Promise<void> => commands.reportFrontend(ok, summary),
  onCoreEvent: (handler: (event: CoreEvent) => void): Promise<UnlistenFn> =>
    events.coreEvent.listen((e) => handler(e.payload)),

  /** Spike only: provoke a framework-level rejection to see what `invoke` rejects with. */
  probeFrameworkError: async (): Promise<{ rawType: string; error: IpcError }> => {
    try {
      await invoke("get_message", { query: { accountId: "acc-1", uid: "not-a-number" } });
      return { rawType: "resolved", error: toIpcError("unexpectedly resolved") };
    } catch (e) {
      return { rawType: typeof e, error: toIpcError(e) };
    }
  },
};
