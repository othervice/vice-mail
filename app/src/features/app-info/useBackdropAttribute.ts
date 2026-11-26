import { useEffect } from "react";
import type { WindowBackdrop } from "../../services/ipc";

/**
 * Stage 22 (decision 0079): marks <html> with `data-backdrop="mica"` when Windows 11 draws Mica
 * behind this window. app.css then leaves the page and its chrome (`data-chrome`: frame, icon
 * rail, docked folder column) transparent, unless the system asks for less transparency or
 * forces its own colours. Without Mica the attribute is absent and every surface stays opaque.
 */
export function useBackdropAttribute(backdrop: WindowBackdrop | undefined): void {
  useEffect(() => {
    const root = document.documentElement;
    if (backdrop === "mica") root.dataset["backdrop"] = backdrop;
    else delete root.dataset["backdrop"];
  }, [backdrop]);
}
