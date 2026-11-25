import { ArrowRightLeft, TriangleExclamationPoint, type LucideIcon } from "lucide-react";
import { useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type Ref } from "react";
import { createPortal } from "react-dom";
import { Button } from "./Button";

interface ConfirmDialogProps {
  title: string;
  body: string;
  confirmLabel: string;
  cancelLabel: string;
  onConfirm: () => void;
  /** Escape, the cancel button or a click beside the dialog. */
  onCancel: () => void;
  /**
   * Where the focus goes after a confirmation (e.g. the next message of the list): the element
   * that opened the dialog may be gone by then. After a cancel it returns to that element.
   */
  focusAfterConfirm?: () => void;
  /**
   * Stage 16: a second way to go on (moving between accounts: "Move" or "Copy"). Shown between
   * Cancel and the confirm button; it counts as a confirmation for the focus.
   */
  secondary?: { label: string; onClick: () => void };
  /** `danger` (default: deleting for good) or `neutral` (a question without loss). */
  tone?: "danger" | "neutral";
  /** Stage 21: the symbol of a neutral question (default: two arrows, moving between accounts). */
  icon?: LucideIcon;
  "data-testid"?: string;
}

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * The one dialog of the app (docs/DESIGN.md: only for deleting for good). An `alertdialog`: the
 * rest of the window is inert while it is open, Tab stays inside, Escape cancels, and the focus
 * returns to where it was. The focus starts on Cancel, the harmless choice. Built by hand
 * instead of Radix Dialog, which inserts a `<style>` element the app's CSP does not allow.
 */
export function ConfirmDialog({
  title,
  body,
  confirmLabel,
  cancelLabel,
  onConfirm,
  onCancel,
  focusAfterConfirm,
  secondary,
  tone = "danger",
  icon: NeutralIcon = ArrowRightLeft,
  ...rest
}: ConfirmDialogProps) {
  const titleId = useId();
  const bodyId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const [host] = useState(() => document.createElement("div"));
  const confirmed = useRef(false);
  const afterConfirm = useRef(focusAfterConfirm);
  const cancel = useRef(onCancel);
  useLayoutEffect(() => {
    afterConfirm.current = focusAfterConfirm;
    cancel.current = onCancel;
  });

  useLayoutEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    document.body.appendChild(host);
    // Everything else becomes inert: no clicks, no focus, hidden from screen readers.
    const others = [...document.body.children].filter(
      (element): element is HTMLElement => element !== host && element instanceof HTMLElement,
    );
    // Attributes, not the `inert` property: they show in the DOM and work in every engine.
    const before = others.map((element) => ({ inert: element.hasAttribute("inert"), hidden: element.getAttribute("aria-hidden") }));
    for (const element of others) {
      element.setAttribute("inert", "");
      element.setAttribute("aria-hidden", "true");
    }
    cancelRef.current?.focus();
    // Escape cancels and the focus stays inside even when the focus was lost to the page (a
    // re-render, the window losing and regaining focus): the dialog's own handler only sees keys
    // pressed inside it.
    const onDocumentKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape" || (event.target instanceof Node && host.contains(event.target))) return;
      event.preventDefault();
      event.stopPropagation();
      cancel.current();
    };
    const onFocusIn = (event: FocusEvent) => {
      if (event.target instanceof Node && !host.contains(event.target)) cancelRef.current?.focus();
    };
    document.addEventListener("keydown", onDocumentKeyDown, true);
    document.addEventListener("focusin", onFocusIn);
    return () => {
      document.removeEventListener("keydown", onDocumentKeyDown, true);
      document.removeEventListener("focusin", onFocusIn);
      others.forEach((element, i) => {
        const was = before[i];
        if (!was?.inert) element.removeAttribute("inert");
        if (was?.hidden == null) element.removeAttribute("aria-hidden");
        else element.setAttribute("aria-hidden", was.hidden);
      });
      host.remove();
      if (confirmed.current && afterConfirm.current) afterConfirm.current();
      else if (previous?.isConnected) previous.focus({ preventScroll: true });
    };
  }, [host]);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onCancel();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = [...(dialogRef.current?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? [])];
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (!first || !last) return;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-scrim px-6"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <div
        ref={dialogRef}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={bodyId}
        onKeyDown={onKeyDown}
        data-testid={rest["data-testid"]}
        className="flex w-full max-w-(--ui-dialog-width) flex-col gap-4 rounded-panel border border-border bg-surface-raised p-6 shadow-panel"
      >
        <div className="flex gap-3">
          {tone === "danger" ? (
            <TriangleExclamationPoint aria-hidden className="size-(--ui-icon) shrink-0 text-danger" strokeWidth={1.75} />
          ) : (
            <NeutralIcon aria-hidden className="size-(--ui-icon) shrink-0 text-accent" strokeWidth={1.75} />
          )}
          <div className="flex min-w-0 flex-col gap-2">
            <h2 id={titleId} className="text-16 font-semibold text-primary">
              {title}
            </h2>
            <p id={bodyId} className="text-14 text-secondary">
              {body}
            </p>
          </div>
        </div>
        <div className="flex justify-end gap-2">
          <CancelButton ref={cancelRef} label={cancelLabel} onClick={onCancel} />
          {secondary && (
            <Button
              variant="secondary"
              onClick={() => {
                confirmed.current = true;
                secondary.onClick();
              }}
              data-testid="confirm-dialog-secondary"
            >
              {secondary.label}
            </Button>
          )}
          <Button
            variant={tone === "danger" ? "danger" : "primary"}
            onClick={() => {
              confirmed.current = true;
              onConfirm();
            }}
            data-testid="confirm-dialog-confirm"
          >
            {confirmLabel}
          </Button>
        </div>
      </div>
    </div>,
    host,
  );
}

function CancelButton({ ref, label, onClick }: { ref: Ref<HTMLButtonElement>; label: string; onClick: () => void }) {
  return (
    <button
      ref={ref}
      type="button"
      onClick={onClick}
      data-testid="confirm-dialog-cancel"
      className="inline-flex h-8 shrink-0 items-center justify-center gap-2 rounded-control border border-border bg-surface-raised px-3 text-13 font-medium text-primary shadow-row transition-colors hover:bg-hover"
    >
      {label}
    </button>
  );
}
