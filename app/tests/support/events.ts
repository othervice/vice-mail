import { act } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import type { CoreEvent } from "../../src/services/ipc";

/** The core sends an event; React state updates it causes are flushed. */
export async function emitCore(event: CoreEvent): Promise<void> {
  await act(async () => {
    await emit("core-event", event);
  });
}
