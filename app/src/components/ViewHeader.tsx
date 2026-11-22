import type { ReactNode } from "react";

interface ViewHeaderProps {
  title: string;
  /** Second line under the title, e.g. the account a mail view belongs to. */
  subtitle?: ReactNode;
  /**
   * On macOS the header doubles as the window's title bar: dragging it (anywhere except on a
   * control) moves the window, a double click zooms it. Tauri's value "deep" covers the subtree.
   */
  dragRegion: boolean;
  children?: ReactNode;
}

export function ViewHeader({ title, subtitle, dragRegion, children }: ViewHeaderProps) {
  return (
    <header
      data-tauri-drag-region={dragRegion ? "deep" : undefined}
      className="flex h-(--ui-header-height) shrink-0 items-center justify-between gap-4 border-b border-border ps-(--ui-header-inset-start) pe-6"
    >
      <div className="flex min-w-0 flex-col">
        <h1 className="truncate text-20 font-semibold text-primary">{title}</h1>
        {subtitle && <div className="flex min-w-0 items-center gap-2 text-12 text-secondary">{subtitle}</div>}
      </div>
      {children}
    </header>
  );
}
