import type { ReactNode } from "react";
import { ToolbarPortal } from "./ToolbarPortal";

interface ViewHeaderProps {
  title: string;
  /** Next to the title, e.g. the account a mail view belongs to or what the view is about. */
  subtitle?: ReactNode;
  /** Stage 23, step 3: right after the title, e.g. the inbox's scope switcher and "18 unread". */
  after?: ReactNode;
  children?: ReactNode;
}

/**
 * The head of a view. Stage 23, step 3 (decision 0084): it lives in the toolbar across the window,
 * over the list, as in the draft: the title (13 px semibold), what belongs to it, then the view's
 * controls at the segment's end. Where there is no toolbar it is a bar of its own above the view.
 */
export function ViewHeader({ title, subtitle, after, children }: ViewHeaderProps) {
  const content = (
    <>
      <div className="flex min-w-0 shrink items-center gap-2">
        <h1 className="min-w-0 truncate text-13 font-semibold text-primary">{title}</h1>
        {after}
        {subtitle && <div className="flex min-w-0 items-center gap-2 text-12 text-tertiary">{subtitle}</div>}
      </div>
      <span className="min-w-0 flex-1" />
      {children}
    </>
  );
  return (
    <ToolbarPortal
      slot="list"
      fallback={
        <header className="flex h-(--ui-toolbar-height) shrink-0 items-center gap-2 border-b border-border-subtle bg-toolbar px-4">{content}</header>
      }
    >
      <div className="flex h-full min-w-0 flex-1 items-center gap-2" data-testid="view-header">
        {content}
      </div>
    </ToolbarPortal>
  );
}
