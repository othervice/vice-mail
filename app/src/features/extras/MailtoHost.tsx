import { Mail } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { extrasIpc, toCoreError, type CoreError } from "../../services/ipc";
import { openComposer, useComposerStore } from "../../stores/composer";
import { useMailtoStore } from "../../stores/mailto";
import { useUiStore } from "../../stores/ui";
import { useSelectedAccount } from "../accounts/selection";
import { composerSourceFor } from "./mailto";

/**
 * Opens `mailto:` links the system handed to the app: the ones it was started with
 * (`takeLaunchLinks`, once) and later ones (`composeRequested`, queued by the event handler).
 * The oldest opens as a new draft from the selected account as soon as there is an account and
 * no other message is being written; meanwhile a notice says that links wait. Nothing is sent:
 * the user reviews the draft (blind copies and ignored parts are pointed out) and sends it.
 */
export function MailtoHost() {
  const { t } = useTranslation();
  const pending = useMailtoStore((state) => state.pending);
  const session = useComposerStore((state) => state.session);
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const { accounts, account } = useSelectedAccount();
  const [launchError, setLaunchError] = useState<CoreError | null>(null);
  const taken = useRef(false);

  // Once per start: the core hands the launch links out only once, so the answer is kept even
  // if React re-runs the effect (StrictMode).
  useEffect(() => {
    if (taken.current) return;
    taken.current = true;
    extrasIpc.takeLaunchLinks().then(
      (requests) => {
        useMailtoStore.getState().enqueue(requests);
      },
      (e: unknown) => {
        setLaunchError(toCoreError(e));
      },
    );
  }, []);

  const composing = session !== null;
  useEffect(() => {
    if (!account || composing || pending.length === 0) return;
    const next = useMailtoStore.getState().shift();
    if (next) openComposer(composerSourceFor(next.request, account.id));
  }, [account, composing, pending]);

  const noAccount = accounts.isSuccess && accounts.data.length === 0;
  const waiting = pending.length > 0 && (noAccount || (account !== null && composing));
  if (!launchError && !waiting) return null;

  return (
    <div className="pointer-events-none fixed end-6 top-(--ui-header-height) z-30 mt-2 flex w-(--ui-notice-width) max-w-full flex-col gap-2">
      {launchError && (
        <div className="pointer-events-auto rounded-card bg-surface-raised shadow-floating">
          <ErrorNotice
            context={t("extras.mailto.failed")}
            error={launchError}
            actions={
              <Button
                variant="secondary"
                onClick={() => {
                  setLaunchError(null);
                }}
              >
                {t("extras.tools.dismiss")}
              </Button>
            }
            data-testid="mailto-launch-error"
          />
        </div>
      )}
      {waiting && (
        <div
          role="status"
          className="pointer-events-auto flex items-start gap-3 rounded-card border border-border bg-surface-raised px-4 py-3 shadow-floating"
          data-testid="mailto-waiting"
        >
          <Mail aria-hidden className="mt-1 size-(--ui-icon-small) shrink-0 text-accent" strokeWidth={1.75} />
          <div className="flex min-w-0 flex-1 flex-col gap-2">
            <p className="text-13 font-semibold text-primary">
              {noAccount ? t("extras.mailto.noAccount.title") : t("extras.mailto.waiting", { count: pending.length })}
            </p>
            <p className="text-13 text-secondary">{noAccount ? t("extras.mailto.noAccount.body") : t("extras.mailto.waitingBody")}</p>
            <div className="flex flex-wrap gap-2">
              {noAccount ? (
                <Button
                  onClick={() => {
                    showAccountsPage({ kind: "add" });
                  }}
                  data-testid="mailto-add-account"
                >
                  {t("extras.mailto.noAccount.add")}
                </Button>
              ) : (
                <Button
                  onClick={() => {
                    // The open draft is saved and stays in Drafts (the composer does that when replaced).
                    const next = useMailtoStore.getState().shift();
                    if (next && account) openComposer(composerSourceFor(next.request, account.id));
                  }}
                  data-testid="mailto-open-now"
                >
                  {t("extras.mailto.openNow")}
                </Button>
              )}
              <Button
                variant="secondary"
                onClick={() => {
                  useMailtoStore.getState().clear();
                }}
                data-testid="mailto-discard"
              >
                {t("extras.mailto.discard")}
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
