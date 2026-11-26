import type { ReactNode } from "react";
import { cx } from "./cx";

interface ButtonProps {
  children: ReactNode;
  onClick?: () => void;
  variant?: "primary" | "secondary" | "danger" | "quiet";
  type?: "button" | "submit";
  disabled?: boolean;
  /** A fuller name where the visible label needs context, e.g. "Save report.pdf" for "Save". */
  "aria-label"?: string;
  "aria-describedby"?: string;
  /** Stable hook for the desktop tests (labels change with the language). */
  "data-testid"?: string;
}

// Design pass 23.11.2026: primary on the accent fill (blue by default, decision 0053) with a hair of
// elevation, secondary as a raised neutral button with a hairline, quiet as accent text.
const VARIANTS = {
  primary: "bg-accent-fill text-on-accent shadow-row hover:bg-accent-fill/90",
  secondary: "border border-border bg-button text-primary shadow-row hover:bg-hover",
  danger: "bg-danger text-on-danger shadow-row hover:bg-danger/90",
  quiet: "text-accent hover:bg-hover",
} as const;

export function Button({ children, onClick, variant = "primary", type = "button", disabled = false, ...rest }: ButtonProps) {
  return (
    <button
      type={type}
      onClick={onClick}
      disabled={disabled}
      aria-label={rest["aria-label"]}
      aria-describedby={rest["aria-describedby"]}
      data-testid={rest["data-testid"]}
      className={cx(
        "inline-flex h-8 shrink-0 items-center justify-center gap-2 rounded-control px-3 text-13 font-medium transition-colors",
        "disabled:cursor-default disabled:opacity-50",
        VARIANTS[variant],
      )}
    >
      {children}
    </button>
  );
}
