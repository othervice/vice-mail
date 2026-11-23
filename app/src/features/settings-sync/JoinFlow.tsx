import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { TextField } from "../../components/TextField";
import type { AccountId, SettingsSyncPreview, SyncKindCount } from "../../services/ipc";
import { formatSyncTime } from "./format";
import { useJoinSettingsSync, usePreviewSettingsSyncJoin } from "./queries";

/** Kinds of `docs/SYNC.md` with a name in the UI; an unknown one from a newer app shows as is. */
const KINDS = [
  "setting",
  "signature",
  "template",
  "rule",
  "savedView",
  "snooze",
  "pin",
  "account",
  "identity",
  "aiSettings",
  "device",
  "task",
  "senderRule",
  "accountGroup",
  "remoteContent",
  "contact",
] as const;

function KindRow({ count }: { count: SyncKindCount }) {
  const { t } = useTranslation();
  const known = KINDS.find((kind) => kind === count.kind);
  const changes = [
    count.added > 0 && t("settingsSync.preview.added", { count: count.added }),
    count.changed > 0 && t("settingsSync.preview.changed", { count: count.changed }),
    count.removed > 0 && t("settingsSync.preview.removed", { count: count.removed }),
  ].filter(Boolean);
  return (
    <li className="flex flex-wrap justify-between gap-x-4 text-13" data-testid={`sync-preview-kind-${count.kind}`}>
      <span className="font-medium text-primary">{known ? t(`settingsSync.kinds.${known}`) : count.kind}</span>
      <span className="text-secondary">{changes.join(", ")}</span>
    </li>
  );
}

function Preview({ preview }: { preview: SettingsSyncPreview }) {
  const { t, i18n } = useTranslation();
  return (
    <div className="flex flex-col gap-4" data-testid="sync-preview">
      {preview.kinds.length === 0 ? (
        <p className="text-13 text-secondary">{t("settingsSync.preview.nothing")}</p>
      ) : (
        <div className="flex flex-col gap-2">
          <p className="text-13 font-medium text-primary">{t("settingsSync.preview.takeOver")}</p>
          <ul className="flex flex-col gap-1">
            {preview.kinds.map((count) => (
              <KindRow key={count.kind} count={count} />
            ))}
          </ul>
        </div>
      )}
      {preview.newAccounts.length > 0 && (
        <div className="flex flex-col gap-1">
          <p className="text-13 font-medium text-primary">{t("settingsSync.preview.newAccounts")}</p>
          <ul className="flex flex-col gap-1 text-13 text-secondary" data-testid="sync-preview-accounts">
            {preview.newAccounts.map((email) => (
              <li key={email} className="select-text">
                {email}
              </li>
            ))}
          </ul>
          <p className="text-12 text-secondary">{t("settingsSync.preview.passwordsLater")}</p>
        </div>
      )}
      <p className="text-13 text-secondary">{t("settingsSync.preview.uploads", { count: preview.uploads })}</p>
      {preview.devices.length > 0 && (
        <div className="flex flex-col gap-1">
          <p className="text-13 font-medium text-primary">{t("settingsSync.preview.devices")}</p>
          <ul className="flex flex-col gap-1 text-13 text-secondary">
            {preview.devices.map((device) => (
              <li key={device.id}>
                {t("settingsSync.devices.row", {
                  name: device.name,
                  system: device.system,
                  time: formatSyncTime(device.lastSeenAt, i18n.language),
                })}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

interface JoinFlowProps {
  accountId: AccountId;
  /** Shown above the passphrase field, e.g. which account holds the folder. */
  intro: string;
  onCancel?: () => void;
}

/** Joining an existing sync: passphrase, then a preview of what comes over, then the merge. */
export function JoinFlow({ accountId, intro, onCancel }: JoinFlowProps) {
  const { t } = useTranslation();
  const [passphrase, setPassphrase] = useState("");
  const preview = usePreviewSettingsSyncJoin();
  const join = useJoinSettingsSync();

  const check = (event: FormEvent) => {
    event.preventDefault();
    if (!passphrase) return;
    preview.mutate(
      { accountId, passphrase },
      {
        onSuccess: () => {
          // The core holds the key now; the passphrase is not needed any more.
          setPassphrase("");
        },
      },
    );
  };

  if (preview.data) {
    return (
      <div className="flex flex-col gap-4 rounded-card border border-accent p-4" data-testid="sync-join-preview">
        <p className="font-semibold text-primary">{t("settingsSync.preview.title")}</p>
        <Preview preview={preview.data} />
        <div className="flex flex-wrap gap-2">
          <Button
            disabled={join.isPending}
            onClick={() => {
              join.mutate(undefined);
            }}
            data-testid="sync-join-confirm"
          >
            {join.isPending ? t("settingsSync.join.joining") : t("settingsSync.join.confirm")}
          </Button>
          <Button
            variant="secondary"
            disabled={join.isPending}
            onClick={() => {
              preview.reset();
              join.reset();
            }}
            data-testid="sync-join-back"
          >
            {t("settingsSync.join.back")}
          </Button>
        </div>
        {join.isError && <ErrorNotice context={t("settingsSync.join.failed")} error={join.error} />}
      </div>
    );
  }

  return (
    <form noValidate onSubmit={check} className="flex max-w-(--ui-form-max-width) flex-col gap-3" data-testid="sync-join-form">
      <p className="text-13 text-secondary">{intro}</p>
      <TextField
        type="password"
        autoComplete="current-password"
        label={t("settingsSync.join.passphrase")}
        value={passphrase}
        onChange={(next) => {
          setPassphrase(next);
          preview.reset();
        }}
        data-testid="sync-join-passphrase"
      />
      <div className="flex flex-wrap gap-2">
        <Button type="submit" disabled={!passphrase || preview.isPending} data-testid="sync-join-check">
          {preview.isPending ? t("settingsSync.join.checking") : t("settingsSync.join.check")}
        </Button>
        {onCancel && (
          <Button variant="secondary" onClick={onCancel} data-testid="sync-join-cancel">
            {t("settingsSync.join.cancel")}
          </Button>
        )}
      </div>
      {preview.isError && <ErrorNotice context={t("settingsSync.join.checkFailed")} error={preview.error} />}
    </form>
  );
}
