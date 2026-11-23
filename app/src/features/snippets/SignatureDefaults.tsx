import { useId } from "react";
import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { ComposeIdentity, IdentitySignatureDefaults, SignatureId, SignatureInfo } from "../../services/ipc";
import { useSetSignatureDefaults, useSignatureDefaults, useSignatures } from "./queries";

/**
 * The default signatures of one sender address: one for new messages, one for replies and
 * forwards (decision 0023). The composer can still switch per message.
 */
export function SignatureDefaults({ identity }: { identity: ComposeIdentity }) {
  const { t } = useTranslation();
  const newId = useId();
  const replyId = useId();
  const signatures = useSignatures();
  const defaults = useSignatureDefaults(identity.id);
  const set = useSetSignatureDefaults();
  const pool = signatures.data ?? [];
  const stored: IdentitySignatureDefaults = defaults.data ?? {
    identityId: identity.id,
    newSignature: identity.signatureNew,
    replySignature: identity.signatureReply,
  };
  // While saving, show the choice just made.
  const current = set.isPending && set.variables ? { ...stored, ...set.variables } : stored;
  const choose = (patch: { newSignature?: SignatureId | null; replySignature?: SignatureId | null }) => {
    set.mutate({
      identityId: identity.id,
      newSignature: patch.newSignature !== undefined ? patch.newSignature : current.newSignature,
      replySignature: patch.replySignature !== undefined ? patch.replySignature : current.replySignature,
    });
  };
  const label = identity.name ? t("composer.identity", { name: identity.name, email: identity.email }) : identity.email;
  return (
    <fieldset className="flex flex-col gap-2" data-testid="signature-defaults">
      <legend className="pb-1 text-13 font-medium text-primary">{label}</legend>
      <div className="flex flex-wrap gap-4">
        <DefaultSelect
          id={newId}
          label={t("snippets.defaults.newMessages")}
          value={known(current.newSignature, pool)}
          pool={pool}
          disabled={!signatures.data}
          onChange={(value) => {
            choose({ newSignature: value });
          }}
          data-testid="signature-default-new"
        />
        <DefaultSelect
          id={replyId}
          label={t("snippets.defaults.replies")}
          value={known(current.replySignature, pool)}
          pool={pool}
          disabled={!signatures.data}
          onChange={(value) => {
            choose({ replySignature: value });
          }}
          data-testid="signature-default-reply"
        />
      </div>
      {defaults.isError && (
        <ErrorNotice context={t("snippets.defaults.loadFailed")} error={defaults.error} onRetry={() => void defaults.refetch()} />
      )}
      {set.isError && <ErrorNotice context={t("snippets.defaults.saveFailed")} error={set.error} data-testid="signature-defaults-error" />}
    </fieldset>
  );
}

/** A deleted signature that is still a default counts as "none" (decision 0023). */
function known(id: SignatureId | null, pool: readonly SignatureInfo[]): string {
  return id !== null && pool.some((s) => s.id === id) ? id : "";
}

interface DefaultSelectProps {
  id: string;
  label: string;
  value: string;
  pool: readonly SignatureInfo[];
  disabled: boolean;
  onChange: (value: SignatureId | null) => void;
  "data-testid": string;
}

function DefaultSelect({ id, label, value, pool, disabled, onChange, "data-testid": testId }: DefaultSelectProps) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="text-12 text-secondary">
        {label}
      </label>
      <select
        id={id}
        value={value}
        disabled={disabled}
        onChange={(e) => {
          onChange(e.target.value === "" ? null : e.target.value);
        }}
        className="h-8 w-fit min-w-(--ui-list-sender-width) rounded-control border border-border-strong bg-surface-raised px-2 text-13 text-primary"
        data-testid={testId}
      >
        <option value="">{t("snippets.defaults.none")}</option>
        {pool.map((signature) => (
          <option key={signature.id} value={signature.id}>
            {signature.name}
          </option>
        ))}
      </select>
    </div>
  );
}
