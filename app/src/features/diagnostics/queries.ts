import { useMutation } from "@tanstack/react-query";
import { diagnosticsIpc } from "../../services/ipc";

/**
 * Stage 15: "Export diagnostics…". The core asks where to save in the system's dialog; the web
 * view's language goes along as the system's locale.
 */
export function useExportDiagnostics() {
  return useMutation({ mutationFn: () => diagnosticsIpc.export(navigator.language) });
}
