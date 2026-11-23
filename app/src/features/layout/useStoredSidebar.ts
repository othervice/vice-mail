import { useEffect } from "react";
import { useLayoutStore } from "../../stores/layout";
import { useUiStore } from "../../stores/ui";

/** A drag of the column's edge is saved once it rests this long (not at every pointer move). */
export const SIDEBAR_WIDTH_SAVE_DELAY_MS = 400;

/**
 * Stage 8: the sidebar state, which the UI store holds, stored with the layout of this window
 * (the density is applied by the layout store itself). The stored value wins when it changes
 * (load, a refused save); a toggle in the UI is saved. The same for the column's width: the
 * drag handle changes the UI store at once, the layout saves it once the width rests.
 */
export function useStoredSidebar(): void {
  useEffect(() => {
    const follow = () => {
      if (useUiStore.getState().sidebarExpanded !== useLayoutStore.getState().sidebarExpanded) useUiStore.getState().toggleSidebar();
    };
    const followWidth = () => {
      const stored = useLayoutStore.getState().sidebarWidth;
      if (useUiStore.getState().sidebarWidth !== stored) useUiStore.getState().setSidebarWidth(stored);
    };
    follow();
    followWidth();
    let pending: ReturnType<typeof setTimeout> | null = null;
    const offLayout = useLayoutStore.subscribe((state, previous) => {
      if (state.sidebarExpanded !== previous.sidebarExpanded) follow();
      if (state.sidebarWidth !== previous.sidebarWidth && pending === null) followWidth();
    });
    const offUi = useUiStore.subscribe((state, previous) => {
      if (state.sidebarExpanded !== previous.sidebarExpanded && state.sidebarExpanded !== useLayoutStore.getState().sidebarExpanded) {
        useLayoutStore.getState().setLayout({ sidebarExpanded: state.sidebarExpanded });
      }
      if (state.sidebarWidth === previous.sidebarWidth) return;
      if (pending !== null) clearTimeout(pending);
      pending = setTimeout(() => {
        pending = null;
        const width = useUiStore.getState().sidebarWidth;
        if (width !== useLayoutStore.getState().sidebarWidth) useLayoutStore.getState().setLayout({ sidebarWidth: width });
      }, SIDEBAR_WIDTH_SAVE_DELAY_MS);
    });
    return () => {
      if (pending !== null) clearTimeout(pending);
      offLayout();
      offUi();
    };
  }, []);
}
