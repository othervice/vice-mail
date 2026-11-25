import { X } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import {
  MAX_REDIRECT_RECIPIENTS,
  redirectIpc,
  toCoreError,
  type Address,
  type CoreError,
  type IdentityId,
} from "../../services/ipc";
import { useComposerStore } from "../../stores/composer";
import { useRedirectStore, type RedirectTarget } from "../../stores/redirect";
import { useAccounts } from "../accounts/queries";
import { RecipientField } from "../composer/RecipientField";
import { useComposeIdentities } from "../composer/queries";
import { trapTab } from "../keyboard/ShortcutOverview";
import { addressName } from "../reading/format";
import { useMessageView } from "../reading/queries";

/** Stage 19 (decision 0055): the redirect dialog, mounted once per window. */
export function RedirectHost() {
  const target = useRedirectStore((state) => state.target);
  return target ? <RedirectDialog key={`${target.accountId}-${String(target.messageId)}`} target={target} /> : null;
}

/**
 * "Redirect": the message goes unchanged to new recipients with `Resent-*` fields (RFC 5322,
 * 3.6.6). Sender is one of the account's identities, recipients as in the composer (chips,
 * suggestions). It goes through the outbox like a send: the send notice counts down the undo
 * delay and offers "Undo". Escape, Cancel or a click beside it closes it; nothing is sent then.
 */
function RedirectDialog({ target }: { target: RedirectTarget }) {
  const { t } = useTranslation();
  const titleId = useId();
  const introId = useId();
  const identityId = useId();
  const close = useRedirectStore((state) => state.close);
  const track = useComposerStore((state) => state.track);
  const account = useAccounts().data?.find((a) => a.id === target.accountId);
  const identities = useComposeIdentities(target.accountId).data ?? [];
  const view = useMessageView(target.accountId, target.messageId, "auto").data;
  const [identity, setIdentity] = useState<IdentityId | null>(null);
  const [to, setTo] = useState<Address[]>([]);
  const [cc, setCc] = useState<Address[]>([]);
  const [bcc, setBcc] = useState<Address[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<CoreError | null>(null);
  const [hint, setHint] = useState<string | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  // One press of "Redirect" is one request: a retry after an error is a new one.
  const requestId = useRef(crypto.randomUUID());
  const panelRef = useRef<HTMLDivElement>(null);

  const sender = view?.header.from[0];
  const senderName = (sender && addressName(sender)) ?? t("list.unknownSender");
  const chosen = identity ?? identities.find((i) => i.isDefault)?.id ?? null;
  const readOnly = account !== undefined && !account.writesEnabled;

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;
    panelRef.current?.querySelector<HTMLElement>('[data-testid="redirect-to"] input')?.focus();
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !panelRef.current?.contains(event.target)) close();
    };
    const onDocumentKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape" || (event.target instanceof Node && panelRef.current?.contains(event.target))) return;
      event.preventDefault();
      event.stopPropagation();
      close();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onDocumentKeyDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onDocumentKeyDown, true);
      if (opener?.isConnected) opener.focus({ preventScroll: true });
    };
  }, [close]);

  const submit = () => {
    const count = to.length + cc.length + bcc.length;
    if (count === 0) {
      setHint(t("redirect.noRecipients"));
      return;
    }
    if (count > MAX_REDIRECT_RECIPIENTS) {
      setHint(t("redirect.tooMany", { count: MAX_REDIRECT_RECIPIENTS }));
      return;
    }
    setHint(null);
    setError(null);
    setBusy(true);
    redirectIpc
      .redirect({ messageId: target.messageId, identityId: chosen, to, cc, bcc }, requestId.current)
      .then(
        (mail) => {
          track(mail);
          close();
        },
        (e: unknown) => {
          const failure = toCoreError(e);
          requestId.current = crypto.randomUUID();
          setRefused(failure.code === "invalid_address" ? (failure.detail ?? null) : null);
          setError(failure);
          setBusy(false);
        },
      );
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape" && !event.defaultPrevented) {
      event.preventDefault();
      close();
      return;
    }
    trapTab(event);
  };

  return (
    <>
      <div aria-hidden className="fixed inset-0 z-40 bg-scrim" data-testid="redirect-backdrop" />
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={introId}
        onKeyDown={onKeyDown}
        data-testid="redirect-dialog"
        className="fixed top-(--ui-palette-top) left-1/2 z-50 flex max-h-[calc(100%-2*var(--ui-palette-top))] w-(--ui-dialog-width) max-w-[calc(100%-2rem)] -translate-x-1/2 flex-col gap-4 rounded-panel border border-border bg-surface-raised p-6 shadow-panel"
      >
        <div className="flex items-start justify-between gap-2">
          <div className="flex min-w-0 flex-col gap-1">
            <h2 id={titleId} className="text-16 font-semibold text-primary">
              {t("redirect.title")}
            </h2>
            <p id={introId} className="text-13 text-secondary">
              {t("redirect.intro", { sender: senderName })}
            </p>
          </div>
          <button
            type="button"
            aria-label={t("redirect.close")}
            onClick={close}
            className="-me-2 -mt-1 flex size-8 shrink-0 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
          >
            <X aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
          </button>
        </div>
        {readOnly && (
          <p role="status" className="text-13 text-warning" data-testid="redirect-read-only">
            {t("redirect.readOnly")}
          </p>
        )}
        <div className="-mx-4 flex min-h-0 flex-col overflow-y-auto">
          {identities.length > 1 && (
            <div className="flex items-center gap-2 border-b border-border-subtle px-4 py-1">
              <label htmlFor={identityId} className="flex h-8 w-12 shrink-0 items-center text-13 text-tertiary">
                {t("redirect.from")}
              </label>
              <select
                id={identityId}
                value={chosen ?? ""}
                onChange={(e) => {
                  setIdentity(Number(e.target.value));
                }}
                className="h-8 min-w-0 flex-1 rounded-control bg-transparent text-13 text-primary"
                data-testid="redirect-identity"
              >
                {identities.map((i) => (
                  <option key={i.id} value={i.id}>
                    {i.name ? `${i.name} <${i.email}>` : i.email}
                  </option>
                ))}
              </select>
            </div>
          )}
          <RecipientField
            label={t("redirect.to")}
            addresses={to}
            onChange={setTo}
            onBlur={() => undefined}
            refused={refused}
            disabled={busy}
            data-testid="redirect-to"
          />
          <RecipientField
            label={t("redirect.cc")}
            addresses={cc}
            onChange={setCc}
            onBlur={() => undefined}
            refused={refused}
            disabled={busy}
            data-testid="redirect-cc"
          />
          <RecipientField
            label={t("redirect.bcc")}
            addresses={bcc}
            onChange={setBcc}
            onBlur={() => undefined}
            refused={refused}
            disabled={busy}
            data-testid="redirect-bcc"
          />
        </div>
        {hint && (
          <p role="alert" className="text-13 text-danger" data-testid="redirect-hint">
            {hint}
          </p>
        )}
        {error && <ErrorNotice context={t("redirect.failed")} error={error} data-testid="redirect-error" />}
        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={close} data-testid="redirect-cancel">
            {t("redirect.cancel")}
          </Button>
          <Button onClick={submit} disabled={busy || readOnly} data-testid="redirect-submit">
            {t("redirect.submit")}
          </Button>
        </div>
      </div>
    </>
  );
}
