import { Tooltip as RadixTooltip } from "radix-ui";
import type { ReactElement } from "react";

interface TooltipProps {
  label: string;
  side?: "top" | "right" | "bottom" | "left";
  children: ReactElement;
}

/** Shows `label` next to a control on hover and keyboard focus. The control keeps its own name. */
export function Tooltip({ label, side = "bottom", children }: TooltipProps) {
  return (
    <RadixTooltip.Root>
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
