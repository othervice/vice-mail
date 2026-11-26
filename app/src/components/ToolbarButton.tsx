import type { LucideIcon } from "lucide-react";
import type { ComponentPropsWithoutRef } from "react";
import { cx } from "./cx";
import { Tooltip } from "./Tooltip";

interface ToolbarButtonProps extends Omit<ComponentPropsWithoutRef<"button">, "children" | "aria-label"> {
  icon: LucideIcon;
  /** The button's name. */
  label: string;
  /** Tooltip, when it says more than the name (the shortcut). */
  hint?: string;
  /** A framed button on the `button` surface (the draft's sidebar toggle, New mail, panel toggle). */
  framed?: boolean;
  /** Shown as on: pressed, unfolded or filtering. */
  active?: boolean;
  "data-testid"?: string;
}

/**
 * Stage 23, step 3 (decision 0084): a button of the toolbar across the window, 28 px high as in
 * the draft: framed (32 × 28, `border` on `button`) or plain (28 × 28). Icons 16 px.
 */
export function ToolbarButton({ icon: Icon, label, hint, framed = false, active = false, className, disabled, ...rest }: ToolbarButtonProps) {
  return (
    <Tooltip label={hint ?? label}>
      <button
        type="button"
        aria-label={label}
        disabled={disabled}
        {...rest}
        className={cx(
          "flex h-(--ui-toolbar-control) shrink-0 items-center justify-center rounded-control transition-colors disabled:cursor-default disabled:opacity-50",
          framed ? "w-8 border border-border bg-button" : "w-(--ui-toolbar-control)",
          active ? "bg-hover text-accent" : "text-secondary hover:text-primary",
          !framed && !active && "hover:bg-hover",
          className,
        )}
      >
        <Icon aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
      </button>
    </Tooltip>
  );
}
