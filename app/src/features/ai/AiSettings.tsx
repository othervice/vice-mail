import type { TFunction } from "i18next";
import { Tabs } from "radix-ui";
import { useId, useState, type FormEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ChoiceGroup } from "../../components/ChoiceGroup";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Notice } from "../../components/Notice";
import { SettingsSection } from "../../components/SettingsSection";
import { Switch } from "../../components/Switch";
import { TextField } from "../../components/TextField";
import {
  AI_LIMITS,
  type Account,
  type AiFunction,
  type AiKeyChange,
  type AiPreset,
  type AiProfileInfo,
  type AiProviderKind,
  type AiSettingsInfo,
} from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { useAccounts } from "../accounts/queries";
import { formatMcpTime } from "../mcp/format";
import { InlineConfirm } from "../mcp/InlineConfirm";
import { AiRequestLog } from "./AiRequestLog";
import { isLocal, LocalityBadge } from "./AiTransfer";
import { ConsentStep } from "./ConsentStep";
import {
  useAiModels,
  useAiSettings,
  useDeleteAiProfile,
  useSaveAiProfile,
  useSetAiAccountCloud,
  useSetAiConsent,
  useSetAiEnabled,
  useSetAiFunction,
  useSetOpenReplySettings,
  useTestAiConnection,
} from "./queries";

const KINDS: readonly AiProviderKind[] = ["openai_compatible", "anthropic"];

function Block({ title, description, children, ...rest }: { title: string; description?: string; children: ReactNode; "data-testid"?: string }) {
  return (
    <section className="flex flex-col gap-3 border-t border-border pt-6" aria-label={title} data-testid={rest["data-testid"]}>
      <div className="flex flex-col gap-1">
        <h3 className="text-14 font-semibold text-primary">{title}</h3>
        {description && <p className="text-13 text-secondary">{description}</p>}
      </div>
      {children}
    </section>
  );
}

/** The two ways AI meets the mail in this app, one sentence each, so nobody mixes them up. */
function Difference() {
  const { t } = useTranslation();
  const showSettings = useUiStore((state) => state.showSettings);
  return (
    <div className="flex flex-col gap-2 rounded-card border border-border bg-canvas p-4 text-13 text-secondary" data-testid="ai-difference">
      <p>
        <span className="font-semibold text-primary">{t("ai.settings.difference.inAppTitle")}</span>{" "}
        {t("ai.settings.difference.inApp")}
      </p>
      <p>
        <span className="font-semibold text-primary">{t("ai.settings.difference.mcpTitle")}</span>{" "}
        {t("ai.settings.difference.mcp")}
      </p>
      <div>
        <Button
          variant="quiet"
          onClick={() => {
            showSettings("mcp");
          }}
          data-testid="ai-open-mcp"
        >
          {t("ai.settings.difference.openMcp")}
        </Button>
      </div>
    </div>
  );
}

function MasterSwitch({ settings }: { settings: AiSettingsInfo }) {
  const { t } = useTranslation();
  const setEnabled = useSetAiEnabled();
  return (
    <div className="flex flex-col gap-2">
      <Switch
        label={t("ai.settings.enable.label")}
        description={t("ai.settings.enable.description")}
        checked={settings.enabled}
        disabled={setEnabled.isPending}
        onChange={(next) => {
          setEnabled.mutate(next);
        }}
        data-testid="ai-enabled"
      />
      <p className="text-13 text-secondary" data-testid="ai-state">
        {settings.enabled ? t("ai.settings.enable.on") : t("ai.settings.enable.off")}
      </p>
      {setEnabled.isError && <ErrorNotice context={t("ai.settings.enable.failed")} error={setEnabled.error} />}
    </div>
  );
}

// ---- Profiles ----

interface ProfileDraft {
  id: string | null;
  name: string;
  kind: AiProviderKind;
  baseUrl: string;
  model: string;
  structuredOutput: boolean;
  maxOutputTokens: string;
  /** Typed into the password field; sent once on save, then forgotten. */
  key: string;
  removeKey: boolean;
  hasKey: boolean;
  needsKey: boolean;
}

function draftOf(profile: AiProfileInfo): ProfileDraft {
  return {
    id: profile.id,
    name: profile.name,
    kind: profile.kind,
    baseUrl: profile.baseUrl,
    model: profile.model,
    structuredOutput: profile.structuredOutput,
    maxOutputTokens: profile.maxOutputTokens === null ? "" : String(profile.maxOutputTokens),
    key: "",
    removeKey: false,
    hasKey: profile.hasKey,
    needsKey: false,
  };
}

function draftOfPreset(preset: AiPreset | null): ProfileDraft {
  return {
    id: null,
    name: preset?.name ?? "",
    kind: preset?.kind ?? "openai_compatible",
    baseUrl: preset?.baseUrl ?? "",
    model: "",
    structuredOutput: true,
    maxOutputTokens: "",
    key: "",
    removeKey: false,
    hasKey: false,
    needsKey: preset?.needsKey ?? false,
  };
}

function parseUrl(text: string): URL | null {
  try {
    return new URL(text);
  } catch {
    return null;
  }
}

/** Checks in the UI what the core would refuse anyway, so the message stands at the field. */
function draftErrors(draft: ProfileDraft, t: TFunction) {
  const name = draft.name.trim();
  const url = parseUrl(draft.baseUrl.trim());
  const tokens = draft.maxOutputTokens.trim();
  return {
    name: !name
      ? t("ai.profiles.form.nameRequired")
      : [...name].length > AI_LIMITS.profileNameChars
        ? t("ai.profiles.form.nameTooLong", { max: AI_LIMITS.profileNameChars })
        : null,
    baseUrl:
      !url || (url.protocol !== "https:" && url.protocol !== "http:") || url.username || url.password
        ? t("ai.profiles.form.baseUrlInvalid")
        : null,
    maxOutputTokens: tokens && !/^[1-9]\d{0,6}$/.test(tokens) ? t("ai.profiles.form.maxOutputTokensInvalid") : null,
  };
}

/** The model picker: the provider's own list, asked for only when the user wants it. */
function ModelPicker({ profileId, value, onChange }: { profileId: string; value: string; onChange: (model: string) => void }) {
  const { t } = useTranslation();
  const id = useId();
  const [load, setLoad] = useState(false);
  const models = useAiModels(profileId, load);
  const list = models.data ?? [];
  const known = list.some((model) => model.id === value);
  return (
    <div className="flex flex-col gap-2" data-testid="ai-model-picker">
      <label htmlFor={id} className="text-13 font-medium text-primary">
        {t("ai.profiles.form.model")}
      </label>
      {models.isSuccess ? (
        list.length === 0 ? (
          <p className="text-13 text-secondary">{t("ai.profiles.form.noModels")}</p>
        ) : (
          <select
            id={id}
            value={value}
            onChange={(event) => {
              onChange(event.target.value);
            }}
            data-testid="ai-model-select"
            className="h-8 min-w-0 rounded-control border border-border-strong bg-surface-raised px-2 text-14 text-primary"
          >
            <option value="">{t("ai.profiles.form.chooseModel")}</option>
            {value && !known && <option value={value}>{value}</option>}
            {list.map((model) => (
              <option key={model.id} value={model.id}>
                {model.displayName && model.displayName !== model.id ? `${model.displayName} (${model.id})` : model.id}
              </option>
            ))}
          </select>
        )
      ) : (
        <div className="flex flex-wrap items-center gap-3">
          <p id={id} className="text-13 text-secondary">
            {value || t("ai.profiles.noModel")}
          </p>
          <Button
            variant="secondary"
            disabled={models.isFetching}
            onClick={() => {
              if (load) void models.refetch();
              else setLoad(true);
            }}
            data-testid="ai-load-models"
          >
            {models.isFetching ? t("ai.profiles.form.loadingModels") : t("ai.profiles.form.loadModels")}
          </Button>
        </div>
      )}
      <p className="text-12 text-secondary">{t("ai.profiles.form.modelDescription")}</p>
      {models.isError && (
        <ErrorNotice
          context={t("ai.profiles.form.modelsFailed")}
          error={models.error}
          onRetry={() => {
            void models.refetch();
          }}
        />
      )}
    </div>
  );
}

interface ProfileFormProps {
  initial: ProfileDraft;
  onDone: (createdId: string | null) => void;
  onCancel: () => void;
  existingIds: readonly string[];
}

function ProfileForm({ initial, onDone, onCancel, existingIds }: ProfileFormProps) {
  const { t } = useTranslation();
  const save = useSaveAiProfile();
  const [draft, setDraft] = useState(initial);
  const [showErrors, setShowErrors] = useState(false);
  const errors = draftErrors(draft, t);
  const invalid = Object.values(errors).some(Boolean);
  const update = (patch: Partial<ProfileDraft>) => {
    setDraft((current) => ({ ...current, ...patch }));
    save.reset();
  };
  const submit = (event: FormEvent) => {
    event.preventDefault();
    setShowErrors(true);
    if (invalid) return;
    const key: AiKeyChange = draft.key ? { type: "set", key: draft.key } : draft.removeKey ? { type: "remove" } : { type: "keep" };
    const tokens = draft.maxOutputTokens.trim();
    save.mutate(
      {
        id: draft.id,
        name: draft.name.trim(),
        kind: draft.kind,
        baseUrl: draft.baseUrl.trim(),
        model: draft.model,
        structuredOutput: draft.structuredOutput,
        maxOutputTokens: tokens ? Number(tokens) : null,
        key,
      },
      {
        onSuccess: (settings) => {
          // The key leaves the form's state at once.
          setDraft((current) => ({ ...current, key: "" }));
          const created = draft.id === null ? (settings.profiles.find((p) => !existingIds.includes(p.id))?.id ?? null) : null;
          onDone(created);
        },
      },
    );
  };
  return (
    <form
      noValidate
      onSubmit={submit}
      aria-label={draft.id ? t("ai.profiles.form.editTitle", { name: initial.name }) : t("ai.profiles.form.newTitle")}
      className="flex max-w-(--ui-form-max-width) flex-col gap-4 rounded-card border border-accent p-4"
      data-testid="ai-profile-form"
    >
      <p className="font-semibold text-primary">
        {draft.id ? t("ai.profiles.form.editTitle", { name: initial.name }) : t("ai.profiles.form.newTitle")}
      </p>
      <TextField
        label={t("ai.profiles.form.name")}
        value={draft.name}
        onChange={(name) => {
          update({ name });
        }}
        error={showErrors ? errors.name : null}
        data-testid="ai-profile-name"
      />
      <ChoiceGroup
        label={t("ai.profiles.form.kind")}
        description={t(`ai.profiles.kindDescription.${draft.kind}`)}
        value={draft.kind}
        options={KINDS.map((kind) => ({ value: kind, label: t(`ai.profiles.kinds.${kind}`) }))}
        onChange={(kind) => {
          update({ kind });
        }}
      />
      <TextField
        label={t("ai.profiles.form.baseUrl")}
        description={t("ai.profiles.form.baseUrlDescription")}
        value={draft.baseUrl}
        onChange={(baseUrl) => {
          update({ baseUrl });
        }}
        error={showErrors ? errors.baseUrl : null}
        data-testid="ai-profile-url"
      />
      <TextField
        label={t("ai.profiles.form.key")}
        type="password"
        description={
          draft.hasKey && !draft.removeKey
            ? t("ai.profiles.form.keyStored")
            : draft.needsKey
              ? t("ai.profiles.form.keyNeeded")
              : t("ai.profiles.form.keyOptional")
        }
        value={draft.key}
        onChange={(key) => {
          update({ key });
        }}
        data-testid="ai-profile-key"
      />
      {draft.hasKey && (
        <label className="flex items-center gap-2 text-13 text-primary">
          <input
            type="checkbox"
            checked={draft.removeKey}
            disabled={draft.key !== ""}
            onChange={(event) => {
              update({ removeKey: event.target.checked });
            }}
            data-testid="ai-profile-remove-key"
          />
          {t("ai.profiles.form.removeKey")}
        </label>
      )}
      {draft.id ? (
        <ModelPicker
          profileId={draft.id}
          value={draft.model}
          onChange={(model) => {
            update({ model });
          }}
        />
      ) : (
        <p className="text-13 text-secondary">{t("ai.profiles.form.modelAfterSave")}</p>
      )}
      <Switch
        label={t("ai.profiles.form.structuredOutput")}
        description={t("ai.profiles.form.structuredOutputDescription")}
        checked={draft.structuredOutput}
        onChange={(structuredOutput) => {
          update({ structuredOutput });
        }}
        data-testid="ai-profile-structured"
      />
      <TextField
        label={t("ai.profiles.form.maxOutputTokens")}
        description={t("ai.profiles.form.maxOutputTokensDescription")}
        value={draft.maxOutputTokens}
        narrow
        onChange={(maxOutputTokens) => {
          update({ maxOutputTokens });
        }}
        error={showErrors ? errors.maxOutputTokens : null}
        data-testid="ai-profile-max-tokens"
      />
      <div className="flex flex-wrap gap-2">
        <Button type="submit" disabled={save.isPending} data-testid="ai-profile-save">
          {save.isPending ? t("ai.profiles.form.saving") : t("ai.profiles.form.save")}
        </Button>
        <Button variant="secondary" disabled={save.isPending} onClick={onCancel} data-testid="ai-profile-cancel">
          {t("ai.profiles.form.cancel")}
        </Button>
      </div>
      {save.isError && <ErrorNotice context={t("ai.profiles.form.saveFailed")} error={save.error} />}
    </form>
  );
}

function ConnectionResult({ profileId }: { profileId: string }) {
  const { t } = useTranslation();
  const test = useTestAiConnection();
  const result = test.data;
  return (
    <div className="flex flex-col gap-2">
      <div>
        <Button
          variant="secondary"
          disabled={test.isPending}
          onClick={() => {
            test.mutate(profileId);
          }}
          data-testid="ai-test"
        >
          {test.isPending ? t("ai.profiles.test.running") : t("ai.profiles.test.run")}
        </Button>
      </div>
      {result && (
        <div role="status" className="flex flex-col gap-1 text-13" data-testid="ai-test-result">
          <p className="text-primary">{t("ai.profiles.test.models", { count: result.models })}</p>
          {result.modelListed === false && <p className="text-warning">{t("ai.profiles.test.modelMissing")}</p>}
          {result.chatOk === true && <p className="text-success">{t("ai.profiles.test.chatOk")}</p>}
          {result.chatOk === null && <p className="text-secondary">{t("ai.profiles.test.noModel")}</p>}
          {result.chatOk === false && (
            <p className="text-danger">
              {t("ai.profiles.test.chatFailed", {
                reason: result.chatError ? t(`errors.codes.${result.chatError}.title`) : t("ai.profiles.test.unknown"),
              })}
            </p>
          )}
        </div>
      )}
      {test.isError && <ErrorNotice context={t("ai.profiles.test.failed")} error={test.error} />}
    </div>
  );
}

function ConsentState({ profile }: { profile: AiProfileInfo }) {
  const { t, i18n } = useTranslation();
  const setConsent = useSetAiConsent();
  const [asking, setAsking] = useState(false);
  if (isLocal(profile.locality)) return <p className="text-12 text-secondary">{t("ai.profiles.consent.local")}</p>;
  if (asking && profile.needsConsent) {
    return (
      <ConsentStep
        aiFunction={null}
        profile={profile}
        busy={setConsent.isPending}
        error={setConsent.isError ? setConsent.error : null}
        onAgree={() => {
          setConsent.mutate(
            { profileId: profile.id, granted: true },
            {
              onSuccess: () => {
                setAsking(false);
              },
            },
          );
        }}
        onCancel={() => {
          setConsent.reset();
          setAsking(false);
        }}
      />
    );
  }
  return (
    <div className="flex flex-wrap items-center gap-3 text-12 text-secondary" data-testid="ai-consent-state">
      {profile.consentAt !== null && !profile.needsConsent ? (
        <>
          <span>{t("ai.profiles.consent.given", { time: formatMcpTime(profile.consentAt, i18n.language) })}</span>
          <Button
            variant="quiet"
            disabled={setConsent.isPending}
            onClick={() => {
              setConsent.mutate({ profileId: profile.id, granted: false });
            }}
            aria-label={t("ai.profiles.consent.withdrawLabel", { name: profile.name })}
            data-testid="ai-consent-withdraw"
          >
            {t("ai.profiles.consent.withdraw")}
          </Button>
        </>
      ) : (
        <>
          <span>{t("ai.profiles.consent.missing")}</span>
          <Button
            variant="quiet"
            onClick={() => {
              setAsking(true);
            }}
            aria-label={t("ai.profiles.consent.readLabel", { name: profile.name })}
            data-testid="ai-consent-read"
          >
            {t("ai.profiles.consent.read")}
          </Button>
        </>
      )}
      {setConsent.isError && !asking && <ErrorNotice context={t("ai.consent.failed")} error={setConsent.error} />}
    </div>
  );
}

function ProfileRow({ profile, onEdit }: { profile: AiProfileInfo; onEdit: () => void }) {
  const { t } = useTranslation();
  const remove = useDeleteAiProfile();
  const [confirming, setConfirming] = useState(false);
  return (
    <li className="flex flex-col gap-3 rounded-card border border-border bg-canvas p-3" data-testid={`ai-profile-${profile.id}`}>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <p className="flex flex-wrap items-center gap-2">
            <span className="truncate text-14 font-medium text-primary">{profile.name}</span>
            <LocalityBadge locality={profile.locality} />
          </p>
          <p className="text-12 text-secondary">
            {[
              t(`ai.profiles.kinds.${profile.kind}`),
              profile.host,
              profile.model || t("ai.profiles.noModel"),
              profile.hasKey ? t("ai.profiles.keyStored") : t("ai.profiles.noKey"),
            ].join(" · ")}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="secondary" onClick={onEdit} aria-label={t("ai.profiles.editLabel", { name: profile.name })} data-testid="ai-profile-edit">
            {t("ai.profiles.edit")}
          </Button>
          <Button
            variant="secondary"
            onClick={() => {
              setConfirming(true);
            }}
            aria-label={t("ai.profiles.deleteLabel", { name: profile.name })}
            data-testid="ai-profile-delete"
          >
            {t("ai.profiles.delete")}
          </Button>
        </div>
      </div>
      {confirming && (
        <InlineConfirm
          title={t("ai.profiles.deleteConfirm.title", { name: profile.name })}
          confirmLabel={t("ai.profiles.deleteConfirm.confirm")}
          cancelLabel={t("ai.profiles.deleteConfirm.cancel")}
          busy={remove.isPending}
          onConfirm={() => {
            remove.mutate(profile.id, {
              onSuccess: () => {
                setConfirming(false);
              },
            });
          }}
          onCancel={() => {
            setConfirming(false);
          }}
          footer={remove.isError && <ErrorNotice context={t("ai.profiles.deleteFailed")} error={remove.error} />}
          data-testid="ai-profile-delete-confirm"
        >
          <p>{t("ai.profiles.deleteConfirm.body")}</p>
        </InlineConfirm>
      )}
      <ConsentState profile={profile} />
      <ConnectionResult profileId={profile.id} />
    </li>
  );
}

type Editing = { kind: "none" } | { kind: "new"; preset: AiPreset | null } | { kind: "edit"; profileId: string };

function ProfilesBlock({ settings }: { settings: AiSettingsInfo }) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState<Editing>({ kind: "none" });
  const ids = settings.profiles.map((profile) => profile.id);
  const editingProfile = editing.kind === "edit" ? settings.profiles.find((p) => p.id === editing.profileId) : undefined;
  return (
    <Block title={t("ai.profiles.title")} description={t("ai.profiles.description")} data-testid="ai-profiles">
      {settings.profiles.length === 0 ? (
        <p className="text-13 text-secondary">{t("ai.profiles.empty")}</p>
      ) : (
        <ul className="flex flex-col gap-2" aria-label={t("ai.profiles.title")}>
          {settings.profiles.map((profile) =>
            editingProfile?.id === profile.id ? (
              <li key={profile.id}>
                <ProfileForm
                  initial={draftOf(profile)}
                  existingIds={ids}
                  onDone={() => {
                    setEditing({ kind: "none" });
                  }}
                  onCancel={() => {
                    setEditing({ kind: "none" });
                  }}
                />
              </li>
            ) : (
              <ProfileRow
                key={profile.id}
                profile={profile}
                onEdit={() => {
                  setEditing({ kind: "edit", profileId: profile.id });
                }}
              />
            ),
          )}
        </ul>
      )}
      {editing.kind === "new" ? (
        <ProfileForm
          initial={draftOfPreset(editing.preset)}
          existingIds={ids}
          onDone={(created) => {
            // A new profile has no model yet: its form stays open for choosing one from the list.
            setEditing(created ? { kind: "edit", profileId: created } : { kind: "none" });
          }}
          onCancel={() => {
            setEditing({ kind: "none" });
          }}
        />
      ) : (
        <div className="flex flex-col gap-2" role="group" aria-label={t("ai.profiles.add")}>
          <p className="text-13 font-medium text-primary">{t("ai.profiles.add")}</p>
          <div className="flex flex-wrap gap-2">
            {settings.presets.map((preset) => (
              <Button
                key={preset.name}
                variant="secondary"
                onClick={() => {
                  setEditing({ kind: "new", preset });
                }}
                aria-label={t("ai.profiles.addPreset", {
                  name: preset.name,
                  where: isLocal(preset.locality) ? t("ai.locality.local") : t("ai.locality.cloud"),
                })}
                data-testid={`ai-preset-${preset.name}`}
              >
                {preset.name}
                <span className="text-12 text-secondary">{isLocal(preset.locality) ? t("ai.locality.local") : t("ai.locality.cloud")}</span>
              </Button>
            ))}
            <Button
              variant="secondary"
              onClick={() => {
                setEditing({ kind: "new", preset: null });
              }}
              data-testid="ai-preset-custom"
            >
              {t("ai.profiles.custom")}
            </Button>
          </div>
        </div>
      )}
    </Block>
  );
}

// ---- Functions, accounts, open replies ----

function FunctionChoice({ aiFunction, profileId, profiles }: { aiFunction: AiFunction; profileId: string | null; profiles: readonly AiProfileInfo[] }) {
  const { t } = useTranslation();
  const id = useId();
  const set = useSetAiFunction();
  const chosen = profiles.find((profile) => profile.id === profileId);
  return (
    <div className="flex flex-col gap-1" data-testid={`ai-function-${aiFunction}`}>
      <div className="grid grid-cols-[minmax(0,12rem)_minmax(0,1fr)] items-center gap-x-4">
        <label htmlFor={id} className="text-13 font-medium text-primary">
          {t(`ai.functions.${aiFunction}`)}
        </label>
        <select
          id={id}
          value={profileId ?? ""}
          disabled={set.isPending}
          aria-describedby={`${id}-hint`}
          onChange={(event) => {
            const value = event.target.value;
            set.mutate({ aiFunction, profileId: value === "" ? null : value });
          }}
          data-testid={`ai-function-select-${aiFunction}`}
          className="h-8 min-w-0 rounded-control border border-border-strong bg-surface-raised px-2 text-14 text-primary disabled:opacity-50"
        >
          <option value="">{t("ai.functions.off")}</option>
          {profiles.map((profile) => (
            <option key={profile.id} value={profile.id}>
              {t("ai.functions.option", {
                name: profile.name,
                where: isLocal(profile.locality) ? t("ai.locality.local") : t("ai.locality.cloud"),
              })}
            </option>
          ))}
        </select>
      </div>
      <p id={`${id}-hint`} className="text-12 text-secondary">
        {chosen && !chosen.model ? t("ai.functions.noModel") : t(`ai.functions.description.${aiFunction}`)}
      </p>
      {set.isError && <ErrorNotice context={t("ai.functions.failed")} error={set.error} />}
    </div>
  );
}

function FunctionsBlock({ settings }: { settings: AiSettingsInfo }) {
  const { t } = useTranslation();
  return (
    <Block title={t("ai.functions.title")} description={t("ai.functions.intro")} data-testid="ai-functions">
      {settings.functions.map((choice) => (
        <FunctionChoice key={choice.function} aiFunction={choice.function} profileId={choice.profileId} profiles={settings.profiles} />
      ))}
    </Block>
  );
}

function AccountCloud({ accountId, allowed, account }: { accountId: string; allowed: boolean; account: Account | undefined }) {
  const { t } = useTranslation();
  const set = useSetAiAccountCloud();
  const name = account ? (account.displayName ? t("nav.account", { name: account.displayName, email: account.email }) : account.email) : t("ai.accounts.unknown");
  return (
    <li className="flex flex-col gap-2" data-testid={`ai-account-${accountId}`}>
      <Switch
        label={name}
        description={allowed ? t("ai.accounts.allowed") : t("ai.accounts.notAllowed")}
        checked={allowed}
        disabled={set.isPending}
        onChange={(next) => {
          set.mutate({ accountId, allowed: next });
        }}
        data-testid="ai-account-cloud"
      />
      {set.isError && <ErrorNotice context={t("ai.accounts.failed")} error={set.error} />}
    </li>
  );
}

function AccountsBlock({ settings }: { settings: AiSettingsInfo }) {
  const { t } = useTranslation();
  const accounts = useAccounts().data ?? [];
  return (
    <Block title={t("ai.accounts.title")} description={t("ai.accounts.description")} data-testid="ai-accounts">
      {settings.accounts.length === 0 ? (
        <p className="text-13 text-secondary">{t("ai.accounts.empty")}</p>
      ) : (
        <ul className="flex flex-col gap-4" aria-label={t("ai.accounts.title")}>
          {settings.accounts.map((entry) => (
            <AccountCloud
              key={entry.accountId}
              accountId={entry.accountId}
              allowed={entry.cloudAllowed}
              account={accounts.find((account) => account.id === entry.accountId)}
            />
          ))}
        </ul>
      )}
    </Block>
  );
}

function OpenRepliesBlock({ settings }: { settings: AiSettingsInfo }) {
  const { t } = useTranslation();
  const set = useSetOpenReplySettings();
  const { enabled, days } = settings.openReplies;
  const [text, setText] = useState(String(days));
  const value = Number(text);
  const valid = /^\d{1,2}$/.test(text.trim()) && value >= AI_LIMITS.openReplyDays.min && value <= AI_LIMITS.openReplyDays.max;
  const commit = () => {
    if (!valid || value === days) return;
    set.mutate({ enabled, days: value });
  };
  return (
    <Block title={t("ai.openReplies.title")} description={t("ai.openReplies.description")} data-testid="ai-open-replies">
      <Switch
        label={t("ai.openReplies.enable")}
        description={t("ai.openReplies.enableDescription")}
        checked={enabled}
        disabled={set.isPending}
        onChange={(next) => {
          set.mutate({ enabled: next, days });
        }}
        data-testid="ai-open-replies-enabled"
      />
      <form
        noValidate
        className="flex flex-wrap items-end gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          commit();
        }}
      >
        <TextField
          label={t("ai.openReplies.days")}
          description={t("ai.openReplies.daysDescription", AI_LIMITS.openReplyDays)}
          value={text}
          narrow
          disabled={!enabled || set.isPending}
          onChange={setText}
          onBlur={commit}
          error={valid ? null : t("ai.openReplies.daysInvalid", AI_LIMITS.openReplyDays)}
          data-testid="ai-open-replies-days"
        />
      </form>
      {set.isError && <ErrorNotice context={t("ai.openReplies.failed")} error={set.error} />}
    </Block>
  );
}

function SettingsPage({ settings }: { settings: AiSettingsInfo }) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col gap-6">
      <MasterSwitch settings={settings} />
      {settings.enabled ? (
        <>
          <ProfilesBlock settings={settings} />
          <FunctionsBlock settings={settings} />
          <AccountsBlock settings={settings} />
        </>
      ) : (
        <Notice title={t("ai.settings.offTitle")} data-testid="ai-off-notice">
          <p>{t("ai.settings.offBody")}</p>
        </Notice>
      )}
      <OpenRepliesBlock settings={settings} />
    </div>
  );
}

const PAGES = ["settings", "log"] as const;
type Page = (typeof PAGES)[number];

/** Settings → AI: the optional built-in AI (stage 13), kept apart from AI over MCP (stage 12). */
export function AiSettings() {
  const { t } = useTranslation();
  const settings = useAiSettings();
  const [page, setPage] = useState<Page>("settings");
  return (
    <SettingsSection title={t("ai.settings.title")}>
      <p className="text-14 text-secondary">{t("ai.settings.intro")}</p>
      <Difference />
      {settings.isError && (
        <ErrorNotice
          context={t("ai.settings.loadFailed")}
          error={settings.error}
          onRetry={() => {
            void settings.refetch();
          }}
        />
      )}
      {settings.data && (
        <Tabs.Root
          value={page}
          onValueChange={(next) => {
            const match = PAGES.find((candidate) => candidate === next);
            if (match) setPage(match);
          }}
          className="flex flex-col gap-6"
        >
          <Tabs.List aria-label={t("ai.settings.pages.label")} className="flex gap-1 border-b border-border">
            {PAGES.map((id) => (
              <Tabs.Trigger
                key={id}
                value={id}
                data-testid={`ai-page-${id}`}
                className="-mb-px h-9 border-b-2 border-transparent px-3 text-14 text-secondary transition-colors hover:text-primary data-[state=active]:border-accent data-[state=active]:font-medium data-[state=active]:text-primary"
              >
                {t(`ai.settings.pages.${id}`)}
              </Tabs.Trigger>
            ))}
          </Tabs.List>
          {PAGES.map((id) => (
            <Tabs.Content key={id} value={id}>
              {id === "settings" ? <SettingsPage settings={settings.data} /> : <AiRequestLog />}
            </Tabs.Content>
          ))}
        </Tabs.Root>
      )}
    </SettingsSection>
  );
}
