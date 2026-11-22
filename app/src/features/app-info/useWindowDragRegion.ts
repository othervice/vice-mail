import { useAppInfo } from "./queries";

/**
 * True where the app draws its own title-bar area (macOS, titleBarStyle "Overlay"): headers then
 * carry `data-tauri-drag-region` so the window can be moved by them. Windows keeps its native bar.
 */
export function useWindowDragRegion(): boolean {
  return useAppInfo().data?.platform === "macos";
}
