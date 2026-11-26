import { useEffect, useId, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Notice } from "../../components/Notice";
import type { Account, AccountId, PasswordRequest, PasswordTransfer, SettingsSyncStatus } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { formatSyncTime } from "./format";
import {
  useCancelPasswordTransfer,
  useSettingsSyncStatus,
  useDeclinePasswordRequest,
  useGivePasswords,
  useOpenPasswordRequest,
  useStartPasswordTransfer,
  useSyncSettingsNow,
} from "./queries";

/** How often the open "Give passwords" panel looks for a new request (the core checks faster once one runs). */
export const REQUEST_POLL_MS = 3000;

function emailOf(accounts: readonly Account[], id: AccountId, fallback: string): string {
  return accounts.find((account) => account.id === id)?.email ?? fallback;
}

/** The six digits, large and spaced, read out digit by digit. */
function Code({ code, label }: { code: string; label: string }) {
  const digits = code.replace(/\s/g, "");
  return (
    <p className="flex flex-col gap-1">
      <span className="text-12 text-secondary">{label}</span>
      <span
        role="img"
        aria-label={`${label}: ${digits.split("").join(" ")}`}
        className="font-mono text-20 font-semibold tracking-widest text-primary select-text"
        data-testid="transfer-code"
      >
        {code}
      </span>
    </p>
  );
}

function Panel({ title, children, ...rest }: { title: string; children: ReactNode; "data-testid"?: string }) {
  const titleId = useId();
  return (
    <section
      aria-labelledby={titleId}
      className="flex flex-col gap-3 rounded-card border border-accent p-4"
      data-testid={rest["data-testid"]}
    >
      <p id={titleId} className="font-semibold text-primary">
        {title}
      </p>
      {children}
    </section>
  );
}

// ---- The new device ----

/**
 * Stage 22 (decision 0076): "Take over passwords from another device". Above the password fields
 * of the accounts the sync brought; the request, the code to compare and how it ended.
 */
export function TakeOverPasswords({ transfer, accounts }: { transfer: PasswordTransfer | null; accounts: readonly Account[] }) {
  const { t, i18n } = useTranslation();
  const start = useStartPasswordTransfer();
  const cancel = useCancelPasswordTransfer();
  const unknown = t("settingsSync.unknownAccount");
  const startButton = (label: string) => (
    <Button
      variant="secondary"
      disabled={start.isPending}
      onClick={() => {
        cancel.reset();
        start.mutate(undefined);
      }}
      data-testid="transfer-start"
    >
      {start.isPending ? t("settingsSync.takeOver.starting") : label}
    </Button>
  );
  const cancelButton = (label: string) => (
    <Button
      variant="secondary"
      disabled={cancel.isPending}
      onClick={() => {
        start.reset();
        cancel.mutate(undefined);
      }}
      data-testid="transfer-cancel"
    >
      {label}
    </Button>
  );
  const errors = (
    <>
      {start.isError && <ErrorNotice context={t("settingsSync.takeOver.startFailed")} error={start.error} />}
      {cancel.isError && <ErrorNotice context={t("settingsSync.takeOver.cancelFailed")} error={cancel.error} />}
    </>
  );

  if (!transfer) {
    return (
      <div className="flex max-w-(--ui-form-max-width) flex-col gap-2" data-testid="transfer-offer">
        <p className="text-13 text-secondary">{t("settingsSync.takeOver.hint")}</p>
        <div>{startButton(t("settingsSync.takeOver.action"))}</div>
        {errors}
      </div>
    );
  }

  const device = transfer.otherDevice ?? unknown;
  switch (transfer.state) {
    case "waiting":
      return (
        <Panel title={t("settingsSync.takeOver.waitingTitle")} data-testid="transfer-waiting">
          <p role="status" className="text-13 text-secondary">
            {t("settingsSync.takeOver.waitingBody", { time: formatSyncTime(transfer.expiresAt, i18n.language) })}
          </p>
          <div>{cancelButton(t("settingsSync.takeOver.cancel"))}</div>
          {errors}
        </Panel>
      );
    case "confirm":
      return (
        <Panel title={t("settingsSync.takeOver.codeTitle")} data-testid="transfer-confirm">
          {transfer.code && <Code code={transfer.code} label={t("settingsSync.takeOver.codeLabel")} />}
          <p role="status" className="text-13 text-secondary">
            {t("settingsSync.takeOver.codeBody", { device })}
          </p>
          <div>{cancelButton(t("settingsSync.takeOver.cancel"))}</div>
          {errors}
        </Panel>
      );
    case "done": {
      const skipped = transfer.skipped.map((id) => emailOf(accounts, id, unknown));
      return (
        <div className="flex flex-col gap-2" data-testid="transfer-done">
          <Notice
            role="status"
            tone={transfer.stored.length > 0 ? "success" : "warning"}
            title={
              transfer.stored.length > 0
                ? t("settingsSync.takeOver.doneTitle", { count: transfer.stored.length })
                : t("settingsSync.takeOver.doneNone")
            }
          >
            {transfer.stored.length > 0 && <p>{t("settingsSync.takeOver.doneBody")}</p>}
            {skipped.length > 0 && (
              <>
                <p>{t("settingsSync.takeOver.skipped")}</p>
                <ul className="list-disc ps-5" data-testid="transfer-skipped">
                  {skipped.map((email) => (
                    <li key={email} className="select-text">
                      {email}
                    </li>
                  ))}
                </ul>
              </>
            )}
          </Notice>
          <div>{cancelButton(t("settingsSync.takeOver.close"))}</div>
          {errors}
        </div>
      );
    }
    case "declined":
    case "expired":
    case "failed": {
      const title =
        transfer.state === "declined"
          ? t("settingsSync.takeOver.declinedTitle", { device })
          : transfer.state === "expired"
            ? t("settingsSync.takeOver.expiredTitle")
            : t("settingsSync.takeOver.failedTitle");
      return (
        <div className="flex flex-col gap-2" data-testid={`transfer-${transfer.state}`}>
          <Notice role="status" tone="warning" title={title}>
            {transfer.state === "expired" && <p>{t("settingsSync.takeOver.expiredBody")}</p>}
            {transfer.state === "failed" && <p>{t("settingsSync.takeOver.failedBody")}</p>}
          </Notice>
          <div className="flex flex-wrap gap-2">
            {startButton(t("settingsSync.takeOver.again"))}
            {cancelButton(t("settingsSync.takeOver.close"))}
          </div>
          {errors}
        </div>
      );
    }
  }
}

// ---- The device that has the passwords ----

function RequestCard({ request, accounts }: { request: PasswordRequest; accounts: readonly Account[] }) {
  const { t, i18n } = useTranslation();
  const open = useOpenPasswordRequest();
  const give = useGivePasswords();
  const decline = useDeclinePasswordRequest();
  const legendId = useId();
  const ready = request.accounts.filter((account) => account.state === "ready").map((account) => account.accountId);
  const [chosen, setChosen] = useState<ReadonlySet<AccountId>>(() => new Set(ready));
  const unknown = t("settingsSync.unknownAccount");
  const device = request.deviceName;
  const busy = open.isPending || give.isPending || decline.isPending;
  const declineButton = (label: string) => (
    <Button
      variant="secondary"
      disabled={busy}
      onClick={() => {
        decline.mutate(request.requestId);
      }}
      data-testid="give-decline"
    >
      {label}
    </Button>
  );
  const errors = (
    <>
      {open.isError && <ErrorNotice context={t("settingsSync.give.openFailed")} error={open.error} />}
      {give.isError && <ErrorNotice context={t("settingsSync.give.sendFailed")} error={give.error} />}
      {decline.isError && <ErrorNotice context={t("settingsSync.give.declineFailed")} error={decline.error} />}
    </>
  );

  let body: ReactNode;
  switch (request.state) {
    case "open":
      body = (
        <div className="flex flex-wrap gap-2">
          <Button
            disabled={busy}
            onClick={() => {
              open.mutate(request.requestId);
            }}
            data-testid="give-continue"
          >
            {t("settingsSync.give.continue")}
          </Button>
          {declineButton(t("settingsSync.give.declineShort"))}
        </div>
      );
      break;
    case "connecting":
      body = (
        <p role="status" className="text-13 text-secondary" data-testid="give-connecting">
          {t("settingsSync.give.connecting", { device })}
        </p>
      );
      break;
    case "confirm": {
      const send = () => {
        give.mutate({ requestId: request.requestId, accountIds: ready.filter((id) => chosen.has(id)) });
      };
      body = (
        <div className="flex flex-col gap-3">
          {request.code && <Code code={request.code} label={t("settingsSync.give.codeLabel")} />}
          <p role="status" className="text-14 text-primary">
            {t("settingsSync.give.codeQuestion", { device })}
          </p>
          <fieldset aria-labelledby={legendId} className="flex flex-col gap-1">
            <legend id={legendId} className="mb-1 text-13 font-medium text-primary">
              {t("settingsSync.give.accounts")}
            </legend>
            {request.accounts.map((account) => {
              const email = emailOf(accounts, account.accountId, unknown);
              const enabled = account.state === "ready";
              return (
                <label key={account.accountId} className="flex items-baseline gap-2 text-13">
                  <input
                    type="checkbox"
                    disabled={!enabled || busy}
                    checked={enabled && chosen.has(account.accountId)}
                    onChange={(event) => {
                      const next = new Set(chosen);
                      if (event.target.checked) next.add(account.accountId);
                      else next.delete(account.accountId);
                      setChosen(next);
                    }}
                    data-testid={`give-account-${account.accountId}`}
                  />
                  <span className="text-primary select-text">{email}</span>
                  {account.state !== "ready" && (
                    <span className="text-secondary">{t(`settingsSync.give.accountStates.${account.state}`)}</span>
                  )}
                </label>
              );
            })}
          </fieldset>
          <div className="flex flex-wrap gap-2">
            <Button disabled={busy || ready.every((id) => !chosen.has(id))} onClick={send} data-testid="give-send">
              {give.isPending ? t("settingsSync.give.sending") : t("settingsSync.give.send")}
            </Button>
            {declineButton(t("settingsSync.give.decline"))}
          </div>
          {ready.length > 0 && ready.every((id) => !chosen.has(id)) && (
            <p className="text-12 text-secondary">{t("settingsSync.give.nothingChosen")}</p>
          )}
        </div>
      );
      break;
    }
    case "given":
      body = <Notice role="status" tone="success" title={t("settingsSync.give.given", { device })} />;
      break;
    case "declined":
      body = (
        <p role="status" className="text-13 text-secondary">
          {t("settingsSync.give.declined")}
        </p>
      );
      break;
    case "otherDevice":
      body = (
        <p role="status" className="text-13 text-secondary">
          {t("settingsSync.give.otherDevice")}
        </p>
      );
      break;
    case "failed":
      body = (
        <Notice role="status" tone="warning" title={t("settingsSync.give.failedTitle")}>
          <p>{t("settingsSync.give.failedBody")}</p>
        </Notice>
      );
      break;
  }
  return (
    <li className="flex flex-col gap-3 rounded-card border border-border-subtle bg-surface-raised p-4 shadow-card" data-testid={`give-request-${request.state}`}>
      <div className="flex flex-col gap-0.5">
        <p className="font-semibold text-primary">{t("settingsSync.give.requestFrom", { device })}</p>
        <p className="text-12 text-secondary">{t("settingsSync.give.runsUntil", { time: formatSyncTime(request.expiresAt, i18n.language) })}</p>
      </div>
      {body}
      {errors}
    </li>
  );
}

/**
 * Stage 22: "Give passwords to a new device". Opening it runs a pass at once and then every few
 * seconds until a request shows; a request seen by a normal pass shows as a notice.
 */
export function GivePasswords({ status, accounts }: { status: SettingsSyncStatus; accounts: readonly Account[] }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const syncNow = useSyncSettingsNow();
  const { mutate: poll } = syncNow;
  const requests = status.passwordRequests;
  const waiting = open && requests.length === 0;

  useEffect(() => {
    if (!open) return;
    poll(undefined);
  }, [open, poll]);
  useEffect(() => {
    if (!waiting) return;
    const timer = window.setInterval(() => {
      poll(undefined);
    }, REQUEST_POLL_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, [waiting, poll]);

  // A request this device can act on (a pass found it, or the panel was closed meanwhile).
  const first = requests.find((request) => request.state === "open" || request.state === "connecting" || request.state === "confirm");
  return (
    <section className="flex flex-col gap-3 border-t border-border pt-6" data-testid="sync-give">
      <div className="flex flex-col gap-1">
        <h3 className="text-14 font-semibold text-primary">{t("settingsSync.give.title")}</h3>
        <p className="text-13 text-secondary">{t("settingsSync.give.description")}</p>
      </div>
      {!open && first && (
        <Notice tone="info" title={t("settingsSync.give.noticeTitle", { device: first.deviceName })} data-testid="give-notice">
          <div>
            <Button
              variant="secondary"
              onClick={() => {
                setOpen(true);
              }}
            >
              {t("settingsSync.give.noticeOpen")}
            </Button>
          </div>
        </Notice>
      )}
      {open ? (
        <div className="flex flex-col gap-3 rounded-card border border-accent p-4" data-testid="give-panel">
          {requests.length === 0 ? (
            <p role="status" className="text-13 text-secondary" data-testid="give-waiting">
              {t("settingsSync.give.waiting")}
            </p>
          ) : (
            <ul className="flex flex-col gap-3">
              {requests.map((request) => (
                <RequestCard key={request.requestId} request={request} accounts={accounts} />
              ))}
            </ul>
          )}
          <div>
            <Button
              variant="secondary"
              onClick={() => {
                setOpen(false);
              }}
              data-testid="give-close"
            >
              {t("settingsSync.give.close")}
            </Button>
          </div>
        </div>
      ) : (
        !first && (
          <div>
            <Button
              variant="secondary"
              onClick={() => {
                setOpen(true);
              }}
              data-testid="give-open"
            >
              {t("settingsSync.give.open")}
            </Button>
          </div>
        )
      )}
    </section>
  );
}

/**
 * Stage 22: on the page of an account the sync brought without its password, the way to take it
 * over instead of typing it (Settings → Settings sync).
 */
export function PasswordFromOtherDevice({ account }: { account: Account }) {
  const { t } = useTranslation();
  const status = useSettingsSyncStatus();
  const showSettings = useUiStore((state) => state.showSettings);
  const waits = status.data?.state !== "off" && status.data?.accountsNeedingPassword.includes(account.id);
  if (!waits || account.oauth) return null;
  return (
    <Notice title={t("settings.accounts.detail.fromOtherDevice.title")} data-testid="password-from-other-device">
      <p>{t("settings.accounts.detail.fromOtherDevice.body")}</p>
      <div>
        <Button
          variant="secondary"
          onClick={() => {
            showSettings("sync");
          }}
        >
          {t("settings.accounts.detail.fromOtherDevice.action")}
        </Button>
      </div>
    </Notice>
  );
}
