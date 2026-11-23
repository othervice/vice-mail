import { Info, TriangleExclamationPoint } from "lucide-react";
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
  const Icon = tone === "danger" ? TriangleExclamationPoint : Info;
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
      // Design pass 23.11.2026: a raised card with a bar in the tone at its start edge and the
      // tone's symbol at the title, instead of a coloured frame around the whole text.
      className={cx(
        "relative flex flex-col gap-3 overflow-hidden rounded-card border border-border-subtle bg-surface-raised p-4 ps-5 shadow-card",
        "before:absolute before:inset-y-0 before:start-0 before:w-1",
        tone === "danger" ? "before:bg-danger" : "before:bg-accent",
      )}
    >
      <p id={titleId} className="flex items-start gap-2 font-semibold text-primary">
        <Icon aria-hidden className={cx("size-(--ui-icon) shrink-0", tone === "danger" ? "text-danger" : "text-accent")} strokeWidth={1.75} />
        <span>{title}</span>
      </p>
      <div className="flex flex-col gap-2 ps-8 text-secondary">{children}</div>
      <div className="flex flex-wrap gap-2 ps-8">
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
