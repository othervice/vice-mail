// Comparison only: the same calls with ts-rs types. ts-rs generates types, nothing else, so
// command names, argument keys and the event name are hand-written strings here. A typo or a
// renamed Rust parameter still compiles and only fails at runtime.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { CoreEvent } from "../generated/ts-rs/CoreEvent";
import type { MessageDetail } from "../generated/ts-rs/MessageDetail";
import type { MessageQuery } from "../generated/ts-rs/MessageQuery";
import { toIpcError, type Result } from "./ipc";

// Must match `CoreEvent::NAME` in Rust. Nothing checks this.
const CORE_EVENT_NAME = "core-event";

async function call<T>(pending: Promise<T>): Promise<Result<T>> {
  try {
    return { ok: true, value: await pending };
  } catch (e) {
    return { ok: false, error: toIpcError(e) };
  }
}

export const ipcTsRs = {
  getMessage: (query: MessageQuery): Promise<Result<MessageDetail>> =>
    call(invoke<MessageDetail>("get_message", { query })),
  startDemoSync: (accountId: string): Promise<Result<null>> =>
    call(invoke<null>("start_demo_sync", { accountId })),
  onCoreEvent: (handler: (event: CoreEvent) => void): Promise<UnlistenFn> =>
    listen<CoreEvent>(CORE_EVENT_NAME, (e) => handler(e.payload)),
};
