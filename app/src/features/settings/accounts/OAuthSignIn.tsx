import { LogIn } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../../components/Button";
import { ErrorNotice } from "../../../components/ErrorNotice";
import { Notice } from "../../../components/Notice";
import type { Account, AccountSettings, IpcError, OAuthPageTexts, OAuthProvider } from "../../../services/ipc";
import { useAddOAuthAccount, useCancelOAuthSignIn, useOAuthAvailability, useSignInAgain } from "../../accounts/oauthQueries";

export { oauthProviderFor } from "./oauth";

/** The texts of the page the browser shows when it returns to the app, in the app's language. */
function usePageTexts(): OAuthPageTexts {
  const { t } = useTranslation();
  return {
    title: t("settings.accounts.oauth.page.title"),
    done: t("settings.accounts.oauth.page.done"),
    failed: t("settings.accounts.oauth.page.failed"),
  };
}

/** Waiting for the browser, with the way out. */
function Waiting({ provider, onCancel }: { provider: OAuthProvider; onCancel: () => void }) {
  const { t } = useTranslation();
  return (
    <Notice role="status" title={t(`settings.accounts.oauth.${provider}.waiting`)} data-testid="oauth-waiting">
      <p>{t("settings.accounts.oauth.waitingBody")}</p>
      <div>
        <Button variant="secondary" onClick={onCancel} data-testid="oauth-cancel">
          {t("settings.accounts.oauth.cancel")}
        </Button>
      </div>
    </Notice>
  );
}

/** How a sign-in ended without an account: cancelled by the user (no alarm), or failed with its reason. */
function Ended({ error, context }: { error: IpcError; context: string }) {
  const { t } = useTranslation();
  if (error.code === "oauth_cancelled") {
    return <Notice role="status" title={t("settings.accounts.oauth.cancelled")} data-testid="oauth-cancelled" />;
  }
  return <ErrorNotice context={context} error={error} data-testid="oauth-error" />;
}

function UnavailableNote({ provider }: { provider: OAuthProvider }) {
  const { t } = useTranslation();
  return (
    <Notice tone="warning" title={t(`settings.accounts.oauth.${provider}.unavailableTitle`)} data-testid="oauth-unavailable">
      <p>{t(`settings.accounts.oauth.${provider}.unavailableBody`)}</p>
    </Notice>
  );
}

interface OAuthSetupProps {
  provider: OAuthProvider;
  /** The form's settings; `null` while the address or servers are incomplete. */
  settings: AccountSettings | null;
  onAdded: (account: Account) => void;
}

/**
 * Stage 14 (decision 0042): "Sign in with Microsoft/Google" in the account setup. The core opens
 * the provider's page in the system browser and waits; the account is added read-only once the
 * sign-in and the IMAP login worked. Without a client ID in this build Microsoft is marked as
 * unavailable (a password does not work there) and Google stays with the app password.
 */
export function OAuthSetup({ provider, settings, onAdded }: OAuthSetupProps) {
  const { t } = useTranslation();
  const availability = useOAuthAvailability();
  const add = useAddOAuthAccount();
  const cancel = useCancelOAuthSignIn();
  const texts = usePageTexts();

  if (!availability.isSuccess) return null;
  if (!availability.data[provider]) {
    return provider === "google" ? null : <UnavailableNote provider={provider} />;
  }

  const start = () => {
    if (!settings) return;
    cancel.reset();
    add.mutate({ settings, provider, texts }, { onSuccess: onAdded });
  };

  return (
    <div
      className="flex flex-col gap-3 rounded-card border border-border-subtle bg-surface-raised p-4 shadow-card"
      data-testid="oauth-setup"
    >
      <p className="text-14 font-medium text-primary">{t(`settings.accounts.oauth.${provider}.title`)}</p>
      <p className="text-13 text-secondary">{t(`settings.accounts.oauth.${provider}.body`)}</p>
      {add.isPending ? (
        <Waiting
          provider={provider}
          onCancel={() => {
            cancel.mutate();
          }}
        />
      ) : (
        <div>
          <Button onClick={start} disabled={!settings} data-testid="oauth-sign-in">
            <LogIn aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {t(`settings.accounts.oauth.${provider}.button`)}
          </Button>
        </div>
      )}
      {add.isError && <Ended error={add.error} context={t("settings.accounts.oauth.failed")} />}
    </div>
  );
}

/** Stage 14: in the account's settings, in place of the password, for an account that signs in with its provider. */
export function OAuthSignInAgain({ account }: { account: Account }) {
  const { t } = useTranslation();
  const availability = useOAuthAvailability();
  const again = useSignInAgain();
  const cancel = useCancelOAuthSignIn();
  const texts = usePageTexts();
  const provider = account.oauth;
  if (!provider) return null;
  const available = availability.data?.[provider] ?? false;
  const needed = account.status.error?.code === "oauth_sign_in_required";

  return (
    <section className="flex flex-col gap-3 border-t border-border pt-6" data-testid="oauth-account">
      <div className="flex flex-col gap-1">
        <h3 className="text-14 font-semibold text-primary">{t(`settings.accounts.oauth.${provider}.signedIn`)}</h3>
        <p className="text-13 text-secondary">{t("settings.accounts.oauth.accountBody")}</p>
      </div>
      {availability.isSuccess && !available && <UnavailableNote provider={provider} />}
      {needed && available && !again.isPending && (
        <Notice tone="warning" title={t("settings.accounts.oauth.signInRequired")} data-testid="oauth-sign-in-required">
          <p>{t("settings.accounts.oauth.signInRequiredBody")}</p>
        </Notice>
      )}
      {again.isPending ? (
        <Waiting
          provider={provider}
          onCancel={() => {
            cancel.mutate();
          }}
        />
      ) : (
        available && (
          <div>
            <Button
              variant={needed ? "primary" : "secondary"}
              onClick={() => {
                cancel.reset();
                again.mutate({ accountId: account.id, texts });
              }}
              data-testid="oauth-sign-in-again"
            >
              <LogIn aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
              {t("settings.accounts.oauth.signInAgain")}
            </Button>
          </div>
        )
      )}
      {again.isSuccess && <Notice tone="success" role="status" title={t("settings.accounts.oauth.signedInAgain")} />}
      {again.isError && <Ended error={again.error} context={t("settings.accounts.oauth.failed")} />}
    </section>
  );
}
