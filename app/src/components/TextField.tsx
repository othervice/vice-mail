import { useId, type HTMLInputAutoCompleteAttribute } from "react";
import { cx } from "./cx";

interface TextFieldProps {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: "text" | "email" | "password" | "number";
  description?: string;
  /** Shown under the field and announced with it; marks the field invalid. */
  error?: string | null;
  autoComplete?: HTMLInputAutoCompleteAttribute;
  required?: boolean;
  disabled?: boolean;
  /** Narrow field for short values such as a port. */
  narrow?: boolean;
  min?: number;
  max?: number;
  onBlur?: () => void;
  /** Stage 22: the longest value, in characters. */
  maxLength?: number;
  /** Stage 22: what an empty field stands for (an account's address for its list name). */
  placeholder?: string;
  "data-testid"?: string;
}

/** Labelled single-line input. Description and error are tied to it for screen readers. */
export function TextField({
  label,
  value,
  onChange,
  type = "text",
  description,
  error,
  autoComplete = "off",
  required = false,
  disabled = false,
  narrow = false,
  min,
  max,
  onBlur,
  maxLength,
  placeholder,
  ...rest
}: TextFieldProps) {
  const id = useId();
  const descriptionId = `${id}-description`;
  const errorId = `${id}-error`;
  const describedBy = [description && descriptionId, error && errorId].filter(Boolean).join(" ");
  return (
    <div className={cx("flex flex-col gap-1", narrow ? "w-(--ui-list-sender-width)" : "w-full")}>
      <label htmlFor={id} className="text-13 font-medium text-primary">
        {label}
      </label>
      <input
        id={id}
        type={type}
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
        }}
        onBlur={onBlur}
        autoComplete={autoComplete}
        autoCapitalize="none"
        autoCorrect="off"
        spellCheck={false}
        required={required}
        disabled={disabled}
        min={min}
        max={max}
        maxLength={maxLength}
        placeholder={placeholder}
        aria-invalid={error ? true : undefined}
        aria-describedby={describedBy || undefined}
        data-testid={rest["data-testid"]}
        className={cx(
          "h-8 w-full rounded-control border bg-surface-raised px-3 text-14 text-primary select-text",
          "disabled:opacity-50",
          error ? "border-danger" : "border-border-strong",
        )}
      />
      {description && (
        <p id={descriptionId} className="text-12 text-secondary">
          {description}
        </p>
      )}
      {error && (
        <p id={errorId} className="text-12 text-danger">
          {error}
        </p>
      )}
    </div>
  );
}
