import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ChoiceGroup, type ChoiceOption } from "../../../components/ChoiceGroup";
import { Notice } from "../../../components/Notice";
import { TextField } from "../../../components/TextField";
import type { Security } from "../../../services/ipc";
import { serverErrors, withSecurity, type ServerDraft, type ServerKind, type ServerWarning } from "./serverDraft";

interface ServerFieldsProps {
  kind: ServerKind;
  legend: string;
  draft: ServerDraft;
  onChange: (draft: ServerDraft) => void;
  /** Show what is missing or wrong (after the first attempt to submit). */
  showErrors: boolean;
  note?: string;
  /** Security review N-4: the warning for a server the online search found. */
  warning?: ReactNode;
}

/** Host, port, security and user name of one server (IMAP or SMTP). No plaintext option exists. */
export function ServerFields({ kind, legend, draft, onChange, showErrors, note, warning }: ServerFieldsProps) {
  const { t } = useTranslation();
  const errors = showErrors ? serverErrors(draft) : null;
  const securityOptions: ChoiceOption<Security>[] = [
    { value: "tls", label: t("settings.accounts.setup.securityOptions.tls") },
    { value: "starttls", label: t("settings.accounts.setup.securityOptions.starttls") },
  ];
  return (
    <fieldset className="flex flex-col gap-4 rounded-card border border-border-subtle bg-surface-raised shadow-card p-4" data-testid={`server-${kind}`}>
      <legend className="float-left w-full text-14 font-semibold text-primary">{legend}</legend>
      {note && <p className="text-13 text-secondary">{note}</p>}
      {warning}
      <div className="flex gap-3">
        <TextField
          label={t("settings.accounts.setup.host")}
          value={draft.host}
          onChange={(host) => {
            onChange({ ...draft, host });
          }}
          error={errors?.host ? t("settings.accounts.setup.required") : null}
          data-testid={`${kind}-host`}
        />
        <TextField
          narrow
          label={t("settings.accounts.setup.port")}
          value={draft.port}
          onChange={(port) => {
            onChange({ ...draft, port });
          }}
          error={errors?.port ? t("settings.accounts.setup.invalidPort") : null}
          data-testid={`${kind}-port`}
        />
      </div>
      <ChoiceGroup
        label={t("settings.accounts.setup.security")}
        description={t("settings.accounts.setup.securityDescription")}
        value={draft.security}
        options={securityOptions}
        onChange={(security) => {
          onChange(withSecurity(draft, kind, security));
        }}
      />
      <TextField
        label={t("settings.accounts.setup.username")}
        value={draft.username}
        onChange={(username) => {
          onChange({ ...draft, username });
        }}
        error={errors?.username ? t("settings.accounts.setup.required") : null}
        data-testid={`${kind}-username`}
      />
    </fieldset>
  );
}

interface FoundServerWarningProps {
  warning: ServerWarning;
  /** The address's domain, as typed. */
  domain: string;
  host: string;
}

/**
 * Security review N-4 (RFC 6186, section 6): a server the online search found outside the
 * address's domain, or given as an IP address. Online answers are not verified, so it may be
 * someone else's server that would receive the password.
 */
export function FoundServerWarning({ warning, domain, host }: FoundServerWarningProps) {
  const { t } = useTranslation();
  return (
    <Notice
      tone="warning"
      title={t(`settings.accounts.setup.foundWarning.${warning}.title`, { domain })}
      data-testid="found-server-warning"
    >
      <p>{t(`settings.accounts.setup.foundWarning.${warning}.body`, { host })}</p>
    </Notice>
  );
}
