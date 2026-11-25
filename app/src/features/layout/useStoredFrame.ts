import { useEffect } from "react";
import { useLayoutStore } from "../../stores/layout";
import { useUiStore } from "../../stores/ui";

/** A drag of a handle is saved once the size rests this long (not at every pointer move). */
export const SIZE_SAVE_DELAY_MS = 400;

/** The sizes the drag handles set: the UI store holds them, the layout store saves them. */
const SIZES = ["sidebarWidth", "listWidth", "listHeight"] as const;
type Size = (typeof SIZES)[number];

const SETTERS = {
  sidebarWidth: (value: number) => {
    useUiStore.getState().setSidebarWidth(value);
  },
  listWidth: (value: number) => {
    useUiStore.getState().setListWidth(value);
  },
  listHeight: (value: number) => {
    useUiStore.getState().setListHeight(value);
  },
} satisfies Record<Size, (value: number) => void>;

/**
 * One size, kept in step between the UI store (which a drag handle changes at once) and the
 * window's stored layout: a change in the UI is saved once it rests; the stored value wins when
 * it changes (load, another window, a refused save) and no save is pending.
 */
function followSize(size: Size): () => void {
  const follow = () => {
    const stored = useLayoutStore.getState()[size];
    if (useUiStore.getState()[size] !== stored) SETTERS[size](stored);
  };
  follow();
  let pending: ReturnType<typeof setTimeout> | null = null;
  const offLayout = useLayoutStore.subscribe((state, previous) => {
    if (state[size] !== previous[size] && pending === null) follow();
  });
  const offUi = useUiStore.subscribe((state, previous) => {
    if (state[size] === previous[size]) return;
    if (pending !== null) clearTimeout(pending);
    pending = setTimeout(() => {
      pending = null;
      const value = useUiStore.getState()[size];
      if (value !== useLayoutStore.getState()[size]) useLayoutStore.getState().setLayout({ [size]: value });
    }, SIZE_SAVE_DELAY_MS);
  });
  return () => {
    if (pending !== null) clearTimeout(pending);
    offLayout();
    offUi();
  };
}

/**
 * Stage 8: the sidebar state, which the UI store holds, stored with the layout of this window
 * (the density is applied by the layout store itself). The stored value wins when it changes
 * (load, a refused save); a toggle in the UI is saved. The sizes the drag handles set (the
 * column's width; stage 18: the list's width beside the reading pane and its height above it)
 * change the UI store at once, and the layout saves them once they rest: a drag is one save.
 */
export function useStoredFrame(): void {
  useEffect(() => {
    const follow = () => {
      if (useUiStore.getState().sidebarExpanded !== useLayoutStore.getState().sidebarExpanded) useUiStore.getState().toggleSidebar();
    };
    follow();
    const offLayout = useLayoutStore.subscribe((state, previous) => {
      if (state.sidebarExpanded !== previous.sidebarExpanded) follow();
    });
    const offUi = useUiStore.subscribe((state, previous) => {
      if (state.sidebarExpanded !== previous.sidebarExpanded && state.sidebarExpanded !== useLayoutStore.getState().sidebarExpanded) {
        useLayoutStore.getState().setLayout({ sidebarExpanded: state.sidebarExpanded });
      }
    });
    const offSizes = SIZES.map(followSize);
    return () => {
      offLayout();
      offUi();
      for (const off of offSizes) off();
    };
  }, []);
}
