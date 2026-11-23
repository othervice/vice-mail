import { useEffect, useId, useRef, type ReactNode } from "react";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";

interface InlineConfirmProps {
  title: string;
  children: ReactNode;
  confirmLabel: string;
  cancelLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  /** Danger for what cannot be taken back; accent for a decision that needs reading first. */
  tone?: "danger" | "accent";
  busy?: boolean;
  /** Shown under the buttons, e.g. the error of the confirmed action. */
  footer?: ReactNode;
  "data-testid"?: string;
}

/**
 * Asks in place instead of in a dialog (docs/DESIGN.md keeps dialogs for deleting mail for good):
 * the consequences spelled out, one more deliberate click. The focus moves to the question when
 * it appears, so keyboard and screen reader users land on it.
 */
export function InlineConfirm({
  title,
  children,
  confirmLabel,
  cancelLabel,
  onConfirm,
  onCancel,
  tone = "danger",
  busy = false,
  footer,
  ...rest
}: InlineConfirmProps) {
  const titleId = useId();
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.focus();
  }, []);
  return (
    <div
      ref={ref}
      tabIndex={-1}
      role="group"
      aria-labelledby={titleId}
      data-testid={rest["data-testid"]}
      className={cx("flex flex-col gap-3 rounded-card border p-4", tone === "danger" ? "border-danger" : "border-accent")}
    >
      <p id={titleId} className="font-semibold text-primary">
        {title}
      </p>
      <div className="flex flex-col gap-2 text-secondary">{children}</div>
      <div className="flex flex-wrap gap-2">
        <Button variant={tone === "danger" ? "danger" : "primary"} disabled={busy} onClick={onConfirm} data-testid="inline-confirm">
          {confirmLabel}
        </Button>
        <Button variant="secondary" disabled={busy} onClick={onCancel} data-testid="inline-cancel">
          {cancelLabel}
        </Button>
      </div>
      {footer}
    </div>
  );
}
