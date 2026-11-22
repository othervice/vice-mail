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

const VARIANTS = {
  primary: "bg-accent text-on-accent hover:bg-accent/90",
  secondary: "border border-border-strong bg-surface text-primary hover:bg-hover",
  danger: "bg-danger text-on-accent hover:bg-danger/90",
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
