import { Tooltip as RadixTooltip } from "radix-ui";
import { useState, type ReactElement } from "react";

interface TooltipProps {
  label: string;
  side?: "top" | "right" | "bottom" | "left";
  /**
   * No tooltip, e.g. where the label is visible anyway. The control stays the same element, so
   * switching does not move the keyboard focus.
   */
  disabled?: boolean;
  children: ReactElement;
}

/** Shows `label` next to a control on hover and keyboard focus. The control keeps its own name. */
export function Tooltip({ label, side = "bottom", disabled = false, children }: TooltipProps) {
  const [open, setOpen] = useState(false);
  return (
    <RadixTooltip.Root
      open={open && !disabled}
      // Hover and focus while disabled must not leave `open` set: Radix reports no close for a
      // tooltip it already sees closed, so it would pop up as soon as `disabled` turns false
      // (the rail's tooltips when the sidebar is folded again).
      onOpenChange={(next) => {
        setOpen(next && !disabled);
      }}
    >
      <RadixTooltip.Trigger asChild>{children}</RadixTooltip.Trigger>
      <RadixTooltip.Portal>
        <RadixTooltip.Content
          side={side}
          sideOffset={6}
          className="z-50 rounded-control border border-border bg-surface-raised px-2 py-1 text-12 font-medium text-primary shadow-floating"
        >
          {label}
        </RadixTooltip.Content>
      </RadixTooltip.Portal>
    </RadixTooltip.Root>
  );
}
