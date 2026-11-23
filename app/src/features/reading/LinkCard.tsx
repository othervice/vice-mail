import { ExternalLink, LoaderCircle, TriangleExclamationPoint } from "lucide-react";
import { useEffect, useId, useRef, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { LinkInfo, LinkWarning } from "../../services/ipc";
import { useUiStore, type LinkRequest } from "../../stores/ui";
import { useDescribeLink, useOpenLink } from "./queries";

function WarningText({ warning, info }: { warning: LinkWarning; info: LinkInfo }) {
  const { t } = useTranslation();
  switch (warning.kind) {
    case "domainMismatch":
      return <>{t("reading.link.warnings.domainMismatch", { shown: warning.shownDomain, target: warning.targetDomain })}</>;
    case "confusableHost":
      return <>{t("reading.link.warnings.confusableHost", { ascii: warning.asciiHost })}</>;
    case "ipAddressHost":
      return <>{t("reading.link.warnings.ipAddressHost")}</>;
    case "credentialsInUrl":
      return <>{t("reading.link.warnings.credentialsInUrl")}</>;
    case "unsupportedScheme":
      return <>{t("reading.link.warnings.unsupportedScheme", { scheme: info.scheme })}</>;
  }
}

/**
 * The target with its host emphasised, so the part that decides where the link goes stands out.
 * Only the first occurrence after the scheme is marked; the rest stays plain text.
 */
function Target({ info }: { info: LinkInfo }) {
  const host = info.host;
  const start = host ? info.target.indexOf(host, info.scheme.length) : -1;
  if (!host || start < 0) {
    return <span className="text-primary">{info.target}</span>;
  }
  return (
    <>
      <span className="text-secondary">{info.target.slice(0, start)}</span>
      <mark className="rounded-control bg-accent-soft px-1 font-semibold text-primary" data-testid="link-host">
        {host}
      </mark>
      <span className="text-secondary">{info.target.slice(start + host.length)}</span>
    </>
  );
}

interface LinkCardProps {
  request: LinkRequest;
  /** Back to where the reader was, after the card closes. */
  onClosed: () => void;
  /** Asks the core for the message again (a rendering it no longer knows). Without it no button. */
  onReload?: () => void;
  /** What a forgotten rendering means here (decision 0028: an AI result has no message to show again). */
  staleText?: string;
}

/**
 * The check before a link opens (docs/SPEC.md, "Lesen"; architecture, security boundary 5):
 * the real target from the core, warnings when it does not match what the mail shows, and only
 * then the system browser. A bar in the reading pane, not a dialog: nothing else is blocked, and
 * Radix dialogs insert `<style>` elements the app's CSP refuses.
 */
export function LinkCard({ request, onClosed, onReload, staleText }: LinkCardProps) {
  const { t } = useTranslation();
  const dismissLink = useUiStore((state) => state.dismissLink);
  const info = useDescribeLink(request.renderToken, request.index);
  const open = useOpenLink();
  const cardRef = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const targetId = useId();

  // The card takes the focus (also out of the mail frame), so screen readers announce it and
  // the keyboard can answer it.
  useEffect(() => {
    cardRef.current?.focus();
  }, [request.seq]);

  // A rendering the core no longer knows (it keeps about 32): the message has to be shown anew.
  const stale = info.isError && info.error.code === "not_found";

  const close = () => {
    dismissLink();
    onClosed();
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    event.stopPropagation();
    close();
  };

  let title: string;
  let body;
  if (info.isPending) {
    title = t("reading.link.checking");
    body = <LoaderCircle aria-hidden className="size-(--ui-icon) text-tertiary" strokeWidth={1.75} />;
  } else if (info.isError) {
    title = stale ? t("reading.link.staleTitle") : t("reading.link.failed");
    body = stale ? (
      <p className="text-secondary">{staleText ?? t("reading.link.stale")}</p>
    ) : (
      <ErrorNotice context={t("reading.link.failed")} error={info.error} />
    );
  } else {
    const link = info.data;
    const mailto = link.scheme === "mailto";
    const warned = link.warnings.length > 0;
    title = !link.openable
      ? t("reading.link.notOpenableTitle")
      : mailto
        ? t("reading.link.mailtoTitle")
        : t("reading.link.title");
    body = (
      <>
        <p id={targetId} className="font-mono text-13 break-all select-text" data-testid="link-target">
          <Target info={link} />
        </p>
        {link.text && link.text !== link.target && (
          <p className="text-13 break-words text-secondary">{t("reading.link.linkText", { text: link.text })}</p>
        )}
        {warned && (
          <ul className="flex flex-col gap-2" data-testid="link-warnings">
            {link.warnings.map((warning) => (
              <li key={warning.kind} className="flex gap-2" data-warning={warning.kind}>
                <TriangleExclamationPoint aria-hidden className="size-(--ui-icon-small) shrink-0 text-warning" strokeWidth={1.75} />
                <span className="text-13 text-primary">
                  <WarningText warning={warning} info={link} />
                </span>
              </li>
            ))}
          </ul>
        )}
        <div className="flex flex-wrap gap-2">
          {link.openable && (
            <Button
              variant={warned ? "secondary" : "primary"}
              disabled={open.isPending}
              onClick={() => {
                open.mutate(request, { onSuccess: close });
              }}
              data-testid="link-open"
            >
              <ExternalLink aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
              {warned ? t("reading.link.openAnyway") : mailto ? t("reading.link.openMailto") : t("reading.link.open")}
            </Button>
          )}
          <Button variant={link.openable && !warned ? "secondary" : "primary"} onClick={close} data-testid="link-cancel">
            {link.openable ? t("reading.link.cancel") : t("reading.link.close")}
          </Button>
        </div>
        {open.isError && <ErrorNotice context={t("reading.link.openFailed")} error={open.error} />}
      </>
    );
  }

  return (
    <div
      ref={cardRef}
      tabIndex={-1}
      role="group"
      aria-labelledby={titleId}
      aria-describedby={info.isSuccess ? targetId : undefined}
      onKeyDown={onKeyDown}
      data-testid="link-card"
      className="flex shrink-0 flex-col gap-3 border-t border-border bg-surface-raised px-6 py-4 shadow-panel"
    >
      <p id={titleId} className="font-semibold text-primary" role={info.isPending ? "status" : undefined}>
        {title}
      </p>
      {body}
      {(info.isPending || info.isError) && (
        <div className="flex flex-wrap gap-2">
          {stale && onReload && (
            <Button
              onClick={() => {
                close();
                onReload();
              }}
              data-testid="link-reload"
            >
              {t("reading.link.reload")}
            </Button>
          )}
          <Button variant="secondary" onClick={close} data-testid="link-cancel">
            {t("reading.link.close")}
          </Button>
        </div>
      )}
    </div>
  );
}
