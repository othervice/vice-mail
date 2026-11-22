import { useTranslation } from "react-i18next";
import { ChoiceGroup, type ChoiceOption } from "../../../components/ChoiceGroup";
import { TextField } from "../../../components/TextField";
import type { Security } from "../../../services/ipc";
import { serverErrors, withSecurity, type ServerDraft, type ServerKind } from "./serverDraft";

interface ServerFieldsProps {
  kind: ServerKind;
  legend: string;
  draft: ServerDraft;
  onChange: (draft: ServerDraft) => void;
  /** Show what is missing or wrong (after the first attempt to submit). */
  showErrors: boolean;
  note?: string;
}

/** Host, port, security and user name of one server (IMAP or SMTP). No plaintext option exists. */
export function ServerFields({ kind, legend, draft, onChange, showErrors, note }: ServerFieldsProps) {
  const { t } = useTranslation();
  const errors = showErrors ? serverErrors(draft) : null;
  const securityOptions: ChoiceOption<Security>[] = [
    { value: "tls", label: t("settings.accounts.setup.securityOptions.tls") },
    { value: "starttls", label: t("settings.accounts.setup.securityOptions.starttls") },
  ];
  return (
    <fieldset className="flex flex-col gap-4 rounded-card border border-border p-4" data-testid={`server-${kind}`}>
      <legend className="px-1 text-14 font-semibold text-primary">{legend}</legend>
      {note && <p className="text-13 text-secondary">{note}</p>}
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
