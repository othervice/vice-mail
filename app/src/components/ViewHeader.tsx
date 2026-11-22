import type { ReactNode } from "react";

interface ViewHeaderProps {
  title: string;
  /**
   * On macOS the header doubles as the window's title bar: dragging it (anywhere except on a
   * control) moves the window, a double click zooms it. Tauri's value "deep" covers the subtree.
   */
  dragRegion: boolean;
  children?: ReactNode;
}

export function ViewHeader({ title, dragRegion, children }: ViewHeaderProps) {
  return (
    <header
      data-tauri-drag-region={dragRegion ? "deep" : undefined}
      className="flex h-(--ui-header-height) shrink-0 items-center justify-between gap-4 border-b border-border px-6"
    >
      <h1 className="truncate text-20 font-semibold text-primary">
        {title}
      </h1>
      {children}
    </header>
  );
}
