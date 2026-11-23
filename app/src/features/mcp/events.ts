import type { QueryClient } from "@tanstack/react-query";
import type { CoreEvent } from "../../services/ipc";
import { applyMcpAccess, applyMcpStatus } from "./queries";

/** Stage 12: the access log and the master switch follow the core live. */
export function handleMcpEvent(queryClient: QueryClient, event: CoreEvent): void {
  switch (event.type) {
    case "mcpAccess":
      applyMcpAccess(queryClient, event.entries);
      break;
    case "mcpStatusChanged":
      applyMcpStatus(queryClient, event.status);
      break;
  }
}
