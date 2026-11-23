import { useId } from "react";
import { cx } from "./cx";

interface SwitchProps {
  label: string;
  description?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  "data-testid"?: string;
}

/**
 * An on/off setting: a button with `role="switch"`, labelled and described for screen readers.
 * Native, so the CSP needs nothing extra; Space and Enter toggle it like any button.
 */
export function Switch({ label, description, checked, onChange, disabled = false, ...rest }: SwitchProps) {
  const labelId = useId();
  const descriptionId = useId();
  return (
    <div className="flex items-start justify-between gap-6">
      <div className="flex min-w-0 flex-col gap-1">
        <span id={labelId} className="text-14 font-medium text-primary">
          {label}
        </span>
        {description && (
          <span id={descriptionId} className="text-13 text-secondary">
            {description}
          </span>
        )}
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-labelledby={labelId}
        aria-describedby={description ? descriptionId : undefined}
        disabled={disabled}
        onClick={() => {
          onChange(!checked);
        }}
        data-testid={rest["data-testid"]}
        className={cx(
          // Design pass 23.11.2026: 40 × 24, on the accent when on, a recessed track when off.
          "relative flex h-6 w-10 shrink-0 items-center rounded-full border p-1 transition-colors disabled:opacity-50",
          checked ? "justify-end border-accent bg-accent" : "justify-start border-border-strong bg-track",
        )}
      >
        <span
          aria-hidden
          className={cx("size-4 rounded-full shadow-row transition-colors", checked ? "bg-surface-raised" : "bg-secondary")}
        />
      </button>
    </div>
  );
}
