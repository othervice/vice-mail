import { act } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import type { CoreEvent } from "../../src/services/ipc";

/** The core sends an event; React state updates it causes are flushed. */
export async function emitCore(event: CoreEvent): Promise<void> {
  await act(async () => {
    await emit("core-event", event);
  });
}

/**
 * Several core events back to back, the way the engine sends a burst (`messagesChanged` and
 * `mailboxesChanged` together): no await and no React flush between them, so they reach the
 * listeners within one task however busy the machine is. Awaiting `emitCore` once per event
 * instead lets the wall clock run between them, past the UI's burst window on a loaded machine.
 */
export async function emitCoreBurst(events: readonly CoreEvent[]): Promise<void> {
  await act(async () => {
    await Promise.all(events.map((event) => emit("core-event", event)));
  });
}
