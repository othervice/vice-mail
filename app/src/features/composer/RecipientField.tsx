import { X } from "lucide-react";
import { useId, useRef, useState, type ClipboardEvent, type KeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import type { Address, ContactSuggestion } from "../../services/ipc";
import { Avatar } from "../accounts/Avatar";
import { addAddresses, addressFull, addressLabel, looksLikeAddress, parseAddressList, sameAddress } from "./addresses";
import { useContactSuggestions } from "./queries";

/**
 * Stage 8: the composer's fields have no frame of their own, so their row shows the focus ring
 * while the field in it has the keyboard focus (the chips' buttons keep their own ring).
 */
export const FIELD_ROW_FOCUS =
  "has-[input:focus-visible]:outline-2 has-[input:focus-visible]:-outline-offset-2 has-[input:focus-visible]:outline-focus-ring";

interface RecipientFieldProps {
  /** "To", "Cc", "Bcc": the visible label and the field's name for screen readers. */
  label: string;
  addresses: readonly Address[];
  onChange: (addresses: Address[]) => void;
  onBlur: () => void;
  /** The address the core refused (`invalid_address`), marked until it is changed. */
  refused: string | null;
  autoFocus?: boolean;
  disabled?: boolean;
  /** Controls at the end of the row (e.g. "Cc", "Bcc" to show those fields). */
  trailing?: ReactNode;
  "data-testid"?: string;
}

const suggestionAddress = (s: ContactSuggestion): Address => ({ name: s.name, email: s.email });

/**
 * Recipients as chips. Typing a comma, semicolon or Enter (or leaving the field) turns the text
 * into chips; pasting a list splits it. Backspace in the empty field removes the last chip.
 * Suggestions come from addresses of earlier mail (WAI-ARIA combobox: Up and Down choose, Enter
 * or Tab takes, Escape closes). A chip that cannot be an address is marked; the core checks every
 * address again when the message is sent.
 */
export function RecipientField({
  label,
  addresses,
  onChange,
  onBlur,
  refused,
  autoFocus = false,
  disabled = false,
  trailing,
  ...rest
}: RecipientFieldProps) {
  const { t } = useTranslation();
  const inputId = useId();
  const listId = useId();
  const invalidId = useId();
  const input = useRef<HTMLInputElement>(null);
  const [text, setText] = useState("");
  const [active, setActive] = useState(0);
  const [closed, setClosed] = useState(false);
  const suggestions = useContactSuggestions(text);
  const options = (suggestions.data ?? []).filter((s) => !addresses.some((a) => sameAddress(a, suggestionAddress(s))));
  const listOpen = !closed && text.trim() !== "" && options.length > 0;
  const invalid = addresses.filter((a) => !looksLikeAddress(a.email) || a.email.toLowerCase() === refused?.toLowerCase());

  const commit = (value: string) => {
    const parsed = parseAddressList(value);
    if (parsed.length > 0) onChange(addAddresses(addresses, parsed));
    setText("");
    setActive(0);
  };
  const take = (suggestion: ContactSuggestion) => {
    onChange(addAddresses(addresses, [suggestionAddress(suggestion)]));
    setText("");
    setActive(0);
    input.current?.focus();
  };
  const remove = (address: Address) => {
    onChange(addresses.filter((a) => a !== address));
    input.current?.focus();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (listOpen && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setActive((i) => (i + step + options.length) % options.length);
      return;
    }
    if (listOpen && event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      setClosed(true);
      return;
    }
    const chosen = listOpen ? options[Math.min(active, options.length - 1)] : undefined;
    if ((event.key === "Enter" || (event.key === "Tab" && !event.shiftKey)) && chosen) {
      event.preventDefault();
      take(chosen);
      return;
    }
    if ((event.key === "Enter" || event.key === "," || event.key === ";") && text.trim() !== "") {
      // Enter with Cmd/Ctrl is "send": the text still becomes a chip first (the composer saves after).
      if (event.key !== "Enter" || !(event.metaKey || event.ctrlKey)) event.preventDefault();
      commit(text);
      return;
    }
    if (event.key === "Backspace" && text === "" && addresses.length > 0) {
      event.preventDefault();
      onChange(addresses.slice(0, -1));
    }
  };

  const onPaste = (event: ClipboardEvent<HTMLInputElement>) => {
    const pasted = event.clipboardData.getData("text");
    if (!/[,;\n]/.test(pasted)) return;
    event.preventDefault();
    commit(text + pasted);
  };

  // The first line is 32 px high like the subject's field and the "Cc"/"Bcc" buttons at the end:
  // label, chips and buttons share its middle; further lines of chips go below.
  return (
    <div className={cx("relative flex min-w-0 items-start gap-2 border-b border-border-subtle px-4 py-1", FIELD_ROW_FOCUS)} data-testid={rest["data-testid"]}>
      <label htmlFor={inputId} className="flex h-8 w-12 shrink-0 items-center text-13 text-tertiary">
        {label}
      </label>
      <ul aria-label={label} className="flex min-w-0 flex-1 flex-wrap items-center gap-1 py-1">
        {addresses.map((address) => {
          const bad = invalid.includes(address);
          return (
            <li
              key={address.email.toLowerCase()}
              className={cx(
                "flex max-w-full items-center gap-1 h-6 rounded-full border ps-1 pe-1 text-13",
                bad ? "border-danger text-danger" : "border-border-subtle bg-hover text-primary",
              )}
              title={addressFull(address)}
              data-testid="recipient-chip"
              data-invalid={bad || undefined}
            >
              {/* Stage 17 (decision 0050): the contact's picture, else its initial. */}
              <Avatar name={address.name} email={address.email} accountColor={0} ring={false} size="mini" />
              <span className="truncate">{addressLabel(address)}</span>
              {bad && <span className="sr-only">{t("composer.recipients.invalidChip")}</span>}
              <button
                type="button"
                disabled={disabled}
                aria-label={t("composer.recipients.remove", { address: addressFull(address) })}
                onClick={() => {
                  remove(address);
                }}
                className="flex size-4 shrink-0 items-center justify-center rounded-full text-secondary hover:bg-surface-raised hover:text-primary"
              >
                <X aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
              </button>
            </li>
          );
        })}
        <li className="min-w-12 flex-1">
          <input
            ref={input}
            id={inputId}
            value={text}
            disabled={disabled}
            autoFocus={autoFocus}
            role="combobox"
            aria-expanded={listOpen}
            aria-controls={listId}
            aria-autocomplete="list"
            aria-activedescendant={listOpen ? `${listId}-${String(Math.min(active, options.length - 1))}` : undefined}
            aria-invalid={invalid.length > 0 || undefined}
            aria-describedby={invalid.length > 0 ? invalidId : undefined}
            autoComplete="off"
            spellCheck={false}
            onChange={(e) => {
              const value = e.target.value;
              setClosed(false);
              setActive(0);
              // A comma or semicolon typed (or dropped in) ends an entry.
              if (/[,;]\s*$/.test(value) && !/"[^"]*$/.test(value)) commit(value);
              else setText(value);
            }}
            onKeyDown={onKeyDown}
            onPaste={onPaste}
            onBlur={() => {
              if (text.trim() !== "") commit(text);
              setClosed(true);
              onBlur();
            }}
            className="h-6 w-full min-w-0 bg-transparent text-13 text-primary outline-none"
            data-testid="recipient-input"
          />
        </li>
      </ul>
      {trailing && <div className="flex shrink-0 items-center gap-1">{trailing}</div>}
      {invalid.length > 0 && (
        <p id={invalidId} className="sr-only">
          {t("composer.recipients.invalid", { addresses: invalid.map((a) => a.email).join(", ") })}
        </p>
      )}
      <ul
        id={listId}
        role="listbox"
        aria-label={t("composer.recipients.suggestions")}
        hidden={!listOpen}
        className="absolute inset-x-4 top-full z-20 mt-1 flex max-h-(--ui-picker-list-height) flex-col overflow-y-auto rounded-card border border-border bg-surface-raised py-1 shadow-floating"
        data-testid="recipient-suggestions"
      >
        {listOpen &&
          options.map((option, index) => (
            <li
              key={option.email}
              id={`${listId}-${String(index)}`}
              role="option"
              aria-selected={index === Math.min(active, options.length - 1)}
              // The input keeps the focus; a click takes the option.
              onMouseDown={(e) => {
                e.preventDefault();
              }}
              onClick={() => {
                take(option);
              }}
              className={cx(
                "flex cursor-default flex-col px-3 py-1",
                index === Math.min(active, options.length - 1) ? "bg-selected" : "hover:bg-hover",
              )}
              data-testid="recipient-suggestion"
            >
              <span className="truncate text-13 text-primary">{option.name ?? option.email}</span>
              {option.name && <span className="truncate text-12 text-secondary">{option.email}</span>}
            </li>
          ))}
      </ul>
    </div>
  );
}
