import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ChoiceGroup, type ChoiceOption } from "../../../components/ChoiceGroup";
import { TextField } from "../../../components/TextField";
import type { Protocol } from "../../../services/ipc";
import { serverErrors, type ServerDraft } from "./serverDraft";

interface ProtocolChoiceProps {
  value: Protocol;
  onChange: (protocol: Protocol) => void;
}

/**
 * Stage 14 (decision 0041): IMAP and SMTP, or JMAP. Part of the server settings (the advanced
 * part of the setup); IMAP stays the default, also when the domain offers JMAP too.
 */
export function ProtocolChoice({ value, onChange }: ProtocolChoiceProps) {
  const { t } = useTranslation();
  const options: ChoiceOption<Protocol>[] = [
    { value: "imap", label: t("settings.accounts.setup.jmap.options.imap") },
    { value: "jmap", label: t("settings.accounts.setup.jmap.options.jmap") },
  ];
  return (
    <div data-testid="setup-protocol">
      <ChoiceGroup
        label={t("settings.accounts.setup.jmap.protocol")}
        description={t("settings.accounts.setup.jmap.protocolDescription")}
        value={value}
        options={options}
        onChange={onChange}
      />
    </div>
  );
}

interface JmapFieldsProps {
  draft: ServerDraft;
  onChange: (draft: ServerDraft) => void;
  /** Show what is missing or wrong (after the first attempt to submit). */
  showErrors: boolean;
  /** Security review N-4: the warning for a server the online search found. */
  warning?: ReactNode;
}

/** Host, port and user name of a JMAP server. Always HTTPS, so there is no security choice. */
export function JmapFields({ draft, onChange, showErrors, warning }: JmapFieldsProps) {
  const { t } = useTranslation();
  const errors = showErrors ? serverErrors(draft) : null;
  const host = draft.host.trim() || t("settings.accounts.setup.jmap.hostPlaceholder");
  return (
    <fieldset
      className="flex flex-col gap-4 rounded-card border border-border-subtle bg-surface-raised shadow-card p-4"
      data-testid="server-jmap"
    >
      <legend className="float-left w-full text-14 font-semibold text-primary">
        {t("settings.accounts.setup.jmap.server")}
      </legend>
      <p className="text-13 text-secondary">{t("settings.accounts.setup.jmap.serverNote", { host })}</p>
      {warning}
      <div className="flex gap-3">
        <TextField
          label={t("settings.accounts.setup.host")}
          value={draft.host}
          onChange={(next) => {
            onChange({ ...draft, host: next });
          }}
          error={errors?.host ? t("settings.accounts.setup.required") : null}
          data-testid="jmap-host"
        />
        <TextField
          narrow
          label={t("settings.accounts.setup.port")}
          value={draft.port}
          onChange={(port) => {
            onChange({ ...draft, port });
          }}
          error={errors?.port ? t("settings.accounts.setup.invalidPort") : null}
          data-testid="jmap-port"
        />
      </div>
      <TextField
        label={t("settings.accounts.setup.username")}
        value={draft.username}
        onChange={(username) => {
          onChange({ ...draft, username });
        }}
        error={errors?.username ? t("settings.accounts.setup.required") : null}
        data-testid="jmap-username"
      />
    </fieldset>
  );
}
