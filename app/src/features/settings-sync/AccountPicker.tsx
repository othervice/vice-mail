import { RadioGroup } from "radix-ui";
import { useId } from "react";
import type { AccountId } from "../../services/ipc";

interface AccountPickerProps {
  label: string;
  description?: string;
  value: AccountId;
  options: readonly { value: AccountId; label: string }[];
  onChange: (value: AccountId) => void;
}

/**
 * One account out of several, one per line: addresses are long and there may be many, which a
 * segmented choice cannot hold. Arrow keys move, Space selects (Radix radio group).
 */
export function AccountPicker({ label, description, value, options, onChange }: AccountPickerProps) {
  const labelId = useId();
  const descriptionId = useId();
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-col gap-1">
        <span id={labelId} className="text-14 font-medium text-primary">
          {label}
        </span>
        {description && (
          <span id={descriptionId} className="text-13 text-secondary">
            {description}
          </span>
        )}
      </div>
      <RadioGroup.Root
        value={value}
        onValueChange={(next) => {
          const option = options.find((o) => o.value === next);
          if (option) onChange(option.value);
        }}
        orientation="vertical"
        aria-labelledby={labelId}
        {...(description && { "aria-describedby": descriptionId })}
        className="flex flex-col gap-1"
      >
        {options.map((option) => (
          <RadioGroup.Item
            key={option.value}
            value={option.value}
            className="group flex min-h-8 items-center gap-3 rounded-control px-2 text-left text-14 text-primary transition-colors hover:bg-hover"
          >
            <span
              aria-hidden
              className="flex size-4 shrink-0 items-center justify-center rounded-full border border-border-strong group-data-[state=checked]:border-accent"
            >
              <RadioGroup.Indicator className="size-2 rounded-full bg-accent" />
            </span>
            <span className="min-w-0 truncate">{option.label}</span>
          </RadioGroup.Item>
        ))}
      </RadioGroup.Root>
    </div>
  );
}
