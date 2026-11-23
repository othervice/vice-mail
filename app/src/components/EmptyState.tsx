import type { LucideIcon } from "lucide-react";
import { Button } from "./Button";

interface EmptyStateProps {
  icon: LucideIcon;
  title: string;
  body: string;
  /** The next step, if there is one the user can take right now. */
  action?: { label: string; onClick: () => void };
}

export function EmptyState({ icon: Icon, title, body, action }: EmptyStateProps) {
  return (
    // Decision 0031: a soft icon tile with a hair of elevation, then title and body as one group.
    <div className="flex flex-1 flex-col items-center justify-center gap-2 px-8 py-12 text-center">
      <span className="mb-2 flex size-12 items-center justify-center rounded-panel bg-accent-soft text-accent shadow-row">
        <Icon aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
      </span>
      <h2 className="text-16 font-semibold text-primary">{title}</h2>
      <p className="max-w-(--ui-form-max-width) text-14 text-secondary">{body}</p>
      {action && (
        <div className="pt-4">
          <Button onClick={action.onClick}>{action.label}</Button>
        </div>
      )}
    </div>
  );
}
