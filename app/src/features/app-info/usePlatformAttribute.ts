import { useEffect } from "react";
import type { Platform } from "../../services/ipc";

/**
 * Marks <html> with the platform (`data-platform="macos"`), so the frame can make room for
 * window controls that the platform draws over the web view (see styles/app.css).
 */
export function usePlatformAttribute(platform: Platform | undefined): void {
  useEffect(() => {
    const root = document.documentElement;
    if (platform) root.dataset["platform"] = platform;
    else delete root.dataset["platform"];
  }, [platform]);
}
