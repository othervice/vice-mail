import { useEffect } from "react";
import { useLayoutStore } from "../../stores/layout";
import { useUiStore } from "../../stores/ui";

/**
 * Stage 8: the sidebar state, which the UI store holds, stored with the layout of this window
 * (the density is applied by the layout store itself).
 */
export function useLayoutEffects(): void {
  useEffect(() => {
    const stored = useLayoutStore.getState().sidebarExpanded;
    if (useUiStore.getState().sidebarExpanded !== stored) useUiStore.getState().toggleSidebar();
    return useUiStore.subscribe((state, previous) => {
      if (state.sidebarExpanded !== previous.sidebarExpanded) {
        useLayoutStore.getState().setLayout({ sidebarExpanded: state.sidebarExpanded });
      }
    });
  }, []);
}
