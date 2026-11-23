import { RadioGroup } from "radix-ui";
import { useId } from "react";

export interface ChoiceOption<T extends string> {
  value: T;
  label: string;
  /** Language of the label when it differs from the UI (e.g. "Deutsch"), for screen readers. */
  lang?: string;
}

interface ChoiceGroupProps<T extends string> {
  label: string;
  description?: string;
  value: T;
  options: readonly ChoiceOption<T>[];
  onChange: (value: T) => void;
}

/** Segmented single choice (radio group): arrow keys move between options, Space selects. */
export function ChoiceGroup<T extends string>({ label, description, value, options, onChange }: ChoiceGroupProps<T>) {
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
        orientation="horizontal"
        aria-labelledby={labelId}
        {...(description && { "aria-describedby": descriptionId })}
        // Design pass 23.11.2026: a recessed track; the checked option is a raised thumb with a
        // hairline in the accent (3:1 on the track), its label in the primary text colour.
        className="inline-flex w-fit flex-wrap gap-1 rounded-card bg-track p-1"
      >
        {options.map((option) => (
          <RadioGroup.Item
            key={option.value}
            value={option.value}
            className="h-8 rounded-control px-3 text-13 font-medium text-secondary transition-colors hover:bg-surface-raised/60 hover:text-primary data-[state=checked]:bg-surface-raised data-[state=checked]:text-primary data-[state=checked]:shadow-row data-[state=checked]:ring-1 data-[state=checked]:ring-accent"
          >
            <span {...(option.lang && { lang: option.lang })}>{option.label}</span>
          </RadioGroup.Item>
        ))}
      </RadioGroup.Root>
    </div>
  );
}
