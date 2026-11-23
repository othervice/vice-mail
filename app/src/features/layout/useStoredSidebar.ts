import { useEffect } from "react";
import { useLayoutStore } from "../../stores/layout";
import { useUiStore } from "../../stores/ui";

/**
 * Stage 8: the sidebar state, which the UI store holds, stored with the layout of this window
 * (the density is applied by the layout store itself). The stored value wins when it changes
 * (load, a refused save); a toggle in the UI is saved.
 */
export function useStoredSidebar(): void {
  useEffect(() => {
    const follow = () => {
      if (useUiStore.getState().sidebarExpanded !== useLayoutStore.getState().sidebarExpanded) useUiStore.getState().toggleSidebar();
    };
    follow();
    const offLayout = useLayoutStore.subscribe((state, previous) => {
      if (state.sidebarExpanded !== previous.sidebarExpanded) follow();
    });
    const offUi = useUiStore.subscribe((state, previous) => {
      if (state.sidebarExpanded === previous.sidebarExpanded) return;
      if (state.sidebarExpanded !== useLayoutStore.getState().sidebarExpanded) {
        useLayoutStore.getState().setLayout({ sidebarExpanded: state.sidebarExpanded });
      }
    });
    return () => {
      offLayout();
      offUi();
    };
  }, []);
}
