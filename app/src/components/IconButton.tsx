import type { LucideIcon } from "lucide-react";
import { Tooltip } from "./Tooltip";

interface IconButtonProps {
  icon: LucideIcon;
  label: string;
  onClick?: () => void;
  disabled?: boolean;
  tooltipSide?: "top" | "right" | "bottom" | "left";
}

export function IconButton({ icon: Icon, label, onClick, disabled = false, tooltipSide }: IconButtonProps) {
  return (
    <Tooltip label={label} {...(tooltipSide && { side: tooltipSide })}>
      <button
        type="button"
        aria-label={label}
        disabled={disabled}
        onClick={onClick}
        className="flex size-8 items-center justify-center rounded-control text-secondary transition-colors hover:bg-hover hover:text-primary disabled:cursor-default disabled:opacity-50 disabled:hover:bg-transparent disabled:hover:text-secondary"
      >
        <Icon aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
      </button>
    </Tooltip>
  );
}
