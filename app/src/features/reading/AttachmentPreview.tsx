import { ChevronLeft, ChevronRight, Download, LoaderCircle, SquareArrowOutUpRight, X } from "lucide-react";
import { useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { AttachmentPreview, IpcError, PreviewKind } from "../../services/ipc";
import { useAttachmentPreview } from "./attachmentQueries";
import type { AttachmentOwner } from "./queries";

/** What the dialog needs of an attachment (a card of the reading pane or a pinned row). */
export interface PreviewTarget {
  index: number;
  name: string;
  preview: PreviewKind;
}

interface AttachmentPreviewDialogProps {
  owner: AttachmentOwner;
  attachment: PreviewTarget;
  /** Escape, the close button or a click beside the dialog. The focus returns to the opener. */
  onClose: () => void;
  /** The ways on when there is no preview: the card's own Save and Open (with its warning). */
  onSave: () => void;
  onOpen?: () => void;
}

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * Only images the core encoded itself are shown: PNG or JPEG as base64 `data:` URIs. Anything
 * else (which the core never sends) is treated as an attachment that could not be read.
 */
const SAFE_IMAGE = /^data:image\/(png|jpeg);base64,[A-Za-z0-9+/]+=*$/;

const UNREADABLE: IpcError = {
  code: "preview_damaged",
  message: "The preview is not an image the app shows.",
  detail: null,
};

/**
 * Stage 16 (decision 0036): the preview of an attachment. The core decodes it (images and PDF
 * pages re-encoded as PNG or JPEG, text decoded); this dialog shows images as `<img>` and text
 * as text, never as markup. Modal like the confirmation dialog (built by hand for the CSP, not
 * Radix Dialog): the rest of the window is inert, Tab stays inside, Escape closes and the focus
 * returns to the card that opened it.
 */
export function AttachmentPreviewDialog({ owner, attachment, onClose, onSave, onOpen }: AttachmentPreviewDialogProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const [page, setPage] = useState(0);
  const preview = useAttachmentPreview(owner, attachment.index, page);
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const [host] = useState(() => document.createElement("div"));
  const close = useRef(onClose);
  useLayoutEffect(() => {
    close.current = onClose;
  });
  // The last page shown stays while the next one loads (placeholder data), so nothing jumps.
  const current: AttachmentPreview | undefined = preview.data;

  useLayoutEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    document.body.appendChild(host);
    const others = [...document.body.children].filter(
      (element): element is HTMLElement => element !== host && element instanceof HTMLElement,
    );
    const before = others.map((element) => ({ inert: element.hasAttribute("inert"), hidden: element.getAttribute("aria-hidden") }));
    for (const element of others) {
      element.setAttribute("inert", "");
      element.setAttribute("aria-hidden", "true");
    }
    closeRef.current?.focus();
    const onDocumentKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape" || (event.target instanceof Node && host.contains(event.target))) return;
      event.preventDefault();
      event.stopPropagation();
      close.current();
    };
    const onFocusIn = (event: FocusEvent) => {
      if (event.target instanceof Node && !host.contains(event.target)) closeRef.current?.focus();
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
      if (previous?.isConnected) previous.focus({ preventScroll: true });
    };
  }, [host]);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    // A modal dialog: no key reaches the shortcuts of the view behind it ("e" must not archive).
    event.stopPropagation();
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (current?.kind === "pdf" && !preview.isFetching && (event.key === "PageDown" || event.key === "PageUp")) {
      const next = event.key === "PageDown" ? current.page + 1 : current.page - 1;
      if (next >= 0 && next < current.pages) {
        event.preventDefault();
        setPage(next);
      }
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

  const fallback = (
    <>
      <Button
        variant="secondary"
        onClick={() => {
          onClose();
          onSave();
        }}
        data-testid="preview-save"
      >
        <Download aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
        {t("attachments.preview.save")}
      </Button>
      {onOpen && (
        <Button
          variant="secondary"
          onClick={() => {
            onClose();
            onOpen();
          }}
          data-testid="preview-open"
        >
          <SquareArrowOutUpRight aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t("attachments.preview.open")}
        </Button>
      )}
    </>
  );

  let content;
  if (preview.isError && !preview.isFetching) {
    content = (
      <ErrorNotice context={t("attachments.preview.failed")} error={preview.error} actions={fallback} data-testid="preview-error" />
    );
  } else if (current && (current.kind === "image" || current.kind === "pdf") && !SAFE_IMAGE.test(current.dataUrl)) {
    content = <ErrorNotice context={t("attachments.preview.failed")} error={UNREADABLE} actions={fallback} data-testid="preview-error" />;
  } else if (!current) {
    content = (
      <p role="status" className="flex items-center gap-2 py-8 text-13 text-secondary" data-testid="preview-loading">
        <LoaderCircle aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
        {t("attachments.preview.loading")}
      </p>
    );
  } else if (current.kind === "image") {
    content = (
      <div className="flex min-h-0 flex-col items-center gap-2">
        <img
          src={current.dataUrl}
          alt={attachment.name}
          width={current.width}
          height={current.height}
          className="h-auto max-h-(--ui-preview-max-height) w-auto max-w-full rounded-card border border-border-subtle bg-canvas object-contain"
          data-testid="preview-image"
        />
        {current.scaled && <p className="text-12 text-secondary">{t("attachments.preview.scaled")}</p>}
      </div>
    );
  } else if (current.kind === "pdf") {
    const last = current.pages - 1;
    content = (
      <div className="flex min-h-0 flex-col items-center gap-3">
        <div className="min-h-0 max-w-full overflow-auto">
          <img
            src={current.dataUrl}
            alt={t("attachments.preview.pageAlt", { name: attachment.name, page: current.page + 1 })}
            width={current.width}
            height={current.height}
            className="h-auto max-h-(--ui-preview-max-height) w-auto max-w-full rounded-card border border-border-subtle bg-canvas object-contain"
            data-testid="preview-pdf-page"
          />
        </div>
        <div role="group" aria-label={t("attachments.preview.pages")} className="flex items-center gap-2">
          <Button
            variant="secondary"
            disabled={current.page <= 0 || preview.isFetching}
            onClick={() => {
              setPage(current.page - 1);
            }}
            data-testid="preview-previous"
          >
            <ChevronLeft aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {t("attachments.preview.previousPage")}
          </Button>
          <p aria-live="polite" className="min-w-24 text-center text-13 text-secondary tabular-nums" data-testid="preview-page">
            {t("attachments.preview.page", { page: current.page + 1, pages: current.pages })}
          </p>
          <Button
            variant="secondary"
            disabled={current.page >= last || preview.isFetching}
            onClick={() => {
              setPage(current.page + 1);
            }}
            data-testid="preview-next"
          >
            {t("attachments.preview.nextPage")}
            <ChevronRight aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          </Button>
        </div>
        {current.morePages && (
          <p className="text-12 text-secondary" data-testid="preview-more-pages">
            {t("attachments.preview.morePages")}
          </p>
        )}
      </div>
    );
  } else {
    content = (
      <div className="flex min-h-0 flex-col gap-2">
        {/* Text as text: React escapes it, nothing in it is read as markup. Focusable to scroll. */}
        <pre
          tabIndex={0}
          aria-label={attachment.name}
          className="max-h-(--ui-preview-max-height) min-h-0 overflow-auto rounded-card border border-border-subtle bg-canvas p-3 font-mono text-13 break-words whitespace-pre-wrap text-primary select-text"
          data-testid="preview-text"
        >
          {current.text}
        </pre>
        {current.truncated && (
          <p className="text-12 text-secondary" data-testid="preview-truncated">
            {t("attachments.preview.truncated")}
          </p>
        )}
      </div>
    );
  }

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-canvas/70 px-6 py-6"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-busy={preview.isFetching}
        onKeyDown={onKeyDown}
        data-testid="attachment-preview"
        data-preview-kind={attachment.preview}
        className="flex max-h-full w-full max-w-(--ui-preview-width) flex-col gap-4 rounded-panel border border-border bg-surface-raised p-6 shadow-panel"
      >
        <div className="flex items-center justify-between gap-3">
          <h2 id={titleId} className="min-w-0 truncate text-16 font-semibold text-primary" title={attachment.name}>
            {t("attachments.preview.title", { name: attachment.name })}
          </h2>
          <button
            ref={closeRef}
            type="button"
            aria-label={t("attachments.preview.close")}
            onClick={onClose}
            className="flex size-8 shrink-0 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
            data-testid="preview-close"
          >
            <X aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
          </button>
        </div>
        <div className="flex min-h-0 flex-1 flex-col overflow-auto">{content}</div>
      </div>
    </div>,
    host,
  );
}
