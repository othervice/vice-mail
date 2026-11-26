import { ExternalLink, MailMinus } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Notice } from "../../components/Notice";
import type { AccountId, MessageId, UnsubscribeMethod, UnsubscribeOptions, UnsubscribeOutcome } from "../../services/ipc";
import { openComposer } from "../../stores/composer";
import { composerSourceFor } from "./mailto";
import { useUnsubscribe } from "./queries";

/** The ways a message offers, in the order the bar suggests them (one click first). */
export function unsubscribeMethods(options: UnsubscribeOptions): UnsubscribeMethod[] {
  const methods: UnsubscribeMethod[] = [];
  if (options.oneClickHost) methods.push("oneClick");
  if (options.mailto) methods.push("mailto");
  if (options.web) methods.push("web");
  return methods;
}

interface UnsubscribeBarProps {
  messageId: MessageId;
  /** The account the unsubscribe mail is written from. */
  accountId: AccountId;
  options: UnsubscribeOptions;
}

/**
 * "Unsubscribe" for mail with `List-Unsubscribe` (stage 9). Nothing leaves the computer before
 * the user picked one way and clicked it: one click (an HTTPS POST to the host shown, RFC 8058),
 * a prepared mail (opened in the composer, sent only with "Send"), or the list's page in the
 * browser (its full address shown first). Asked in place, not in a dialog (docs/DESIGN.md).
 */
export function UnsubscribeBar({ messageId, accountId, options }: UnsubscribeBarProps) {
  const { t } = useTranslation();
  const methods = unsubscribeMethods(options);
  const unsubscribe = useUnsubscribe(messageId);
  const [asking, setAsking] = useState(false);
  const [outcome, setOutcome] = useState<UnsubscribeOutcome | null>(null);
  const [lastMethod, setLastMethod] = useState<UnsubscribeMethod | null>(null);
  const openRef = useRef<HTMLButtonElement>(null);
  const groupRef = useRef<HTMLDivElement>(null);
  const titleId = useId();

  useEffect(() => {
    if (asking) groupRef.current?.focus();
  }, [asking]);

  if (methods.length === 0) return null;

  const run = (method: UnsubscribeMethod) => {
    setLastMethod(method);
    unsubscribe.mutate(method, {
      onSuccess: (result) => {
        setAsking(false);
        setOutcome(result);
        if (result.kind === "compose") openComposer(composerSourceFor(result.request, accountId));
      },
    });
  };
  const cancel = () => {
    setAsking(false);
    unsubscribe.reset();
    // Back to the button that opened the question (it is on screen again after this render).
    window.setTimeout(() => openRef.current?.focus(), 0);
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    event.stopPropagation();
    event.preventDefault();
    cancel();
  };

  if (outcome) {
    const host = options.oneClickHost ?? "";
    return (
      <div className="flex flex-col gap-2" data-print-hide data-testid="unsubscribe-result" data-outcome={outcome.kind}>
        <Notice tone="success" role="status" title={t(`extras.unsubscribe.result.${outcome.kind}.title`)}>
          <p>
            {outcome.kind === "requested"
              ? t("extras.unsubscribe.result.requested.body", { host, status: outcome.status })
              : t(`extras.unsubscribe.result.${outcome.kind}.body`)}
          </p>
        </Notice>
      </div>
    );
  }

  if (!asking) {
    return (
      <div className="flex flex-wrap items-center gap-3 rounded-card border border-border bg-canvas px-4 py-2" data-print-hide data-testid="unsubscribe-bar">
        <MailMinus aria-hidden className="size-(--ui-icon-small) shrink-0 text-secondary" strokeWidth={1.75} />
        <p className="me-auto text-13 text-secondary">{t("extras.unsubscribe.bar")}</p>
        <button
          ref={openRef}
          type="button"
          onClick={() => {
            setAsking(true);
          }}
          className="inline-flex h-8 items-center rounded-control border border-border bg-button px-3 text-13 font-medium text-primary shadow-row hover:bg-hover"
          data-testid="unsubscribe-open"
        >
          {t("extras.unsubscribe.open")}
        </button>
      </div>
    );
  }

  const busy = unsubscribe.isPending;
  return (
    <div
      ref={groupRef}
      tabIndex={-1}
      role="group"
      aria-labelledby={titleId}
      aria-busy={busy}
      onKeyDown={onKeyDown}
      className="flex flex-col gap-4 rounded-card border border-accent bg-surface-raised p-4"
      data-print-hide
      data-testid="unsubscribe-ask"
    >
      <div className="flex flex-col gap-1">
        <p id={titleId} className="font-semibold text-primary">
          {t("extras.unsubscribe.title")}
        </p>
        <p className="text-13 text-secondary">{t("extras.unsubscribe.intro")}</p>
      </div>
      <ul aria-label={t("extras.unsubscribe.ways")} className="flex flex-col gap-3">
        {methods.map((method, index) => (
          <li key={method} className="flex flex-col gap-2" data-testid={`unsubscribe-way-${method}`}>
            <p className="text-13 font-semibold text-primary">{t(`extras.unsubscribe.${method}.label`)}</p>
            {method === "oneClick" && (
              <p className="text-13 text-secondary">{t("extras.unsubscribe.oneClick.body", { host: options.oneClickHost ?? "" })}</p>
            )}
            {method === "mailto" && (
              <p className="text-13 text-secondary">{t("extras.unsubscribe.mailto.body", { address: options.mailto ?? "" })}</p>
            )}
            {method === "web" && (
              <>
                <p className="text-13 text-secondary">{t("extras.unsubscribe.web.body")}</p>
                <p className="font-mono text-13 break-all text-primary select-text" data-testid="unsubscribe-web-url">
                  {options.web}
                </p>
              </>
            )}
            <div>
              <Button variant={index === 0 ? "primary" : "secondary"} disabled={busy} onClick={() => run(method)} data-testid={`unsubscribe-${method}`}>
                {method === "web" && <ExternalLink aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />}
                {t(`extras.unsubscribe.${method}.confirm`)}
              </Button>
            </div>
          </li>
        ))}
      </ul>
      {busy && (
        <p role="status" className="text-13 text-secondary">
          {t("extras.unsubscribe.busy")}
        </p>
      )}
      {unsubscribe.isError && (
        <ErrorNotice
          context={t("extras.unsubscribe.failed")}
          error={unsubscribe.error}
          {...(lastMethod && { onRetry: () => run(lastMethod) })}
          data-testid="unsubscribe-error"
        />
      )}
      <div>
        <Button variant="secondary" disabled={busy} onClick={cancel} data-testid="unsubscribe-cancel">
          {t("extras.unsubscribe.cancel")}
        </Button>
      </div>
    </div>
  );
}
