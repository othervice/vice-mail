import type { LucideIcon } from "lucide-react";
import { cx } from "./cx";
import { Tooltip } from "./Tooltip";

interface IconButtonProps {
  icon: LucideIcon;
  label: string;
  onClick?: () => void;
  disabled?: boolean;
  tooltipSide?: "top" | "right" | "bottom" | "left";
  /** Stage 23 (decision 0084): a quiet 28 px button with a 16 px symbol (the reading pane's tools). */
  small?: boolean;
}

export function IconButton({ icon: Icon, label, onClick, disabled = false, tooltipSide, small = false }: IconButtonProps) {
  return (
    <Tooltip label={label} {...(tooltipSide && { side: tooltipSide })}>
      <button
        type="button"
        aria-label={label}
        disabled={disabled}
        onClick={onClick}
        className={cx(
          "flex items-center justify-center rounded-control transition-colors hover:bg-hover hover:text-primary disabled:cursor-default disabled:opacity-50 disabled:hover:bg-transparent",
          small ? "size-(--ui-toolbar-control) text-tertiary disabled:hover:text-tertiary" : "size-8 text-secondary disabled:hover:text-secondary",
        )}
      >
        <Icon aria-hidden className={small ? "size-(--ui-icon-small)" : "size-(--ui-icon)"} strokeWidth={1.75} />
      </button>
    </Tooltip>
  );
}
