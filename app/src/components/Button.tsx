import type { ReactNode } from "react";
import { cx } from "./cx";

interface ButtonProps {
  children: ReactNode;
  onClick: () => void;
  variant?: "primary" | "secondary";
}

const VARIANTS = {
  primary: "bg-accent text-on-accent hover:bg-accent/90",
  secondary: "border border-border-strong bg-surface text-primary hover:bg-hover",
} as const;

export function Button({ children, onClick, variant = "primary" }: ButtonProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cx(
        "inline-flex h-8 items-center justify-center gap-2 rounded-control px-3 text-13 font-medium transition-colors",
        VARIANTS[variant],
      )}
    >
      {children}
    </button>
  );
}
