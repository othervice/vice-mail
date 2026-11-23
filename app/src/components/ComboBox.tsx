import { useId, useState, type KeyboardEvent, type ReactNode } from "react";
import { cx } from "./cx";

export interface ComboOption<T> {
  key: string;
  label: string;
  /** Second line, e.g. the full path of a folder. */
  detail?: string;
  icon?: ReactNode;
  value: T;
}

interface ComboBoxProps<T> {
  label: string;
  query: string;
  onQueryChange: (query: string) => void;
  /** Already filtered by the caller, in the order shown. */
  options: readonly ComboOption<T>[];
  onChoose: (option: ComboOption<T>) => void;
  /** Shown instead of the list when it is empty. */
  emptyText: string;
  description?: string;
  /** Shown under the field and announced with it; marks the field invalid. */
  error?: string | null;
  autoFocus?: boolean;
  listLabel?: string;
  "data-testid"?: string;
}

/**
 * A text field with a list of choices below it (WAI-ARIA combobox with a listbox popup that is
 * always shown): typing filters, Up and Down move the active choice, Enter takes it, a click
 * takes the clicked one. The focus stays in the field. Native elements only; no Radix Popover or
 * Select (they inject styles the CSP does not allow).
 */
export function ComboBox<T>({
  label,
  query,
  onQueryChange,
  options,
  onChoose,
  emptyText,
  description,
  error,
  autoFocus = false,
  listLabel,
  ...rest
}: ComboBoxProps<T>) {
  const id = useId();
  const listId = `${id}-list`;
  const descriptionId = `${id}-description`;
  const errorId = `${id}-error`;
  const [active, setActive] = useState(0);
  const current = Math.min(active, options.length - 1);
  const activeOption = current >= 0 ? options[current] : undefined;
  const optionId = (option: ComboOption<T>) => `${id}-option-${option.key}`;

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    const last = options.length - 1;
    const moves: Record<string, number> = {
      ArrowDown: Math.min(last, current + 1),
      ArrowUp: Math.max(0, current - 1),
      PageDown: Math.min(last, current + 8),
      PageUp: Math.max(0, current - 8),
    };
    const move = moves[event.key];
    if (move !== undefined) {
      event.preventDefault();
      setActive(move);
      document.getElementById(`${id}-option-${options[move]?.key ?? ""}`)?.scrollIntoView?.({ block: "nearest" });
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      if (activeOption) onChoose(activeOption);
    }
  };

  const describedBy = [description && descriptionId, error && errorId].filter(Boolean).join(" ");

  return (
    <div className="flex min-h-0 flex-col gap-2" data-testid={rest["data-testid"]}>
      <label htmlFor={id} className="text-13 font-medium text-primary">
        {label}
      </label>
      <input
        id={id}
        type="text"
        role="combobox"
        aria-expanded
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={activeOption ? optionId(activeOption) : undefined}
        aria-invalid={error ? true : undefined}
        aria-describedby={describedBy || undefined}
        value={query}
        // A picker opens for typing; the field is its whole point.
        autoFocus={autoFocus}
        autoComplete="off"
        autoCapitalize="none"
        autoCorrect="off"
        spellCheck={false}
        onChange={(event) => {
          onQueryChange(event.target.value);
          setActive(0);
        }}
        onKeyDown={onKeyDown}
        className={cx(
          "h-8 w-full rounded-control border bg-surface-raised px-3 text-14 text-primary select-text",
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
      <div
        id={listId}
        role="listbox"
        aria-label={listLabel ?? label}
        className="flex max-h-(--ui-picker-list-height) min-h-0 flex-col overflow-y-auto"
      >
        {options.length === 0 ? (
          <p role="presentation" className="px-2 py-2 text-13 text-secondary">
            {emptyText}
          </p>
        ) : (
          options.map((option, index) => (
            <div
              key={option.key}
              id={optionId(option)}
              role="option"
              aria-selected={index === current}
              // Keeps the focus in the field.
              onMouseDown={(event) => {
                event.preventDefault();
              }}
              onClick={() => {
                onChoose(option);
              }}
              onMouseMove={() => {
                if (index !== current) setActive(index);
              }}
              className={cx(
                "flex min-h-8 items-center gap-2 rounded-control px-2 py-1 text-13",
                index === current ? "bg-selected text-primary" : "text-primary",
              )}
            >
              {option.icon}
              <span className="flex min-w-0 flex-col">
                <span className="truncate">{option.label}</span>
                {option.detail && <span className="truncate text-12 text-secondary">{option.detail}</span>}
              </span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
