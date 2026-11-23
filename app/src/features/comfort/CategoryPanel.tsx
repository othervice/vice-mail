import { ChevronDown, ChevronRight, Tag } from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ChoiceGroup, type ChoiceOption } from "../../components/ChoiceGroup";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Switch } from "../../components/Switch";
import type { CategoryReason, MessageCategory, MessageCategoryInfo, MessageId, SenderKey, SenderKeyKind } from "../../services/ipc";
import { useMessageCategory, useSetSenderCategory, useSetSenderImportant } from "./queries";

export const CATEGORIES: readonly MessageCategory[] = ["person", "notification", "newsletter"];

/** The keys "always as …" can use for this message: address, list, domain (in the core's order). */
export function senderKeys(info: MessageCategoryInfo): SenderKey[] {
  return [info.address, info.list, info.domain].filter((key): key is SenderKey => key !== null);
}

/** The values a reason names, for its sentence (`comfort.reasons.<kind>`). */
export function reasonValues(reason: CategoryReason): Record<string, string | number> {
  switch (reason.kind) {
    case "userRule":
      return { value: reason.key.value };
    case "autoSubmitted":
    case "precedence":
      return { value: reason.value };
    case "autoReply":
    case "report":
    case "campaign":
    case "mailingService":
      return { evidence: reason.evidence };
    case "knownSystem":
      return { system: reason.system, header: reason.header };
    case "discussionList":
      return { list: reason.listId ?? "" };
    case "mailingList":
      return { list: reason.listId };
    case "correspondent":
      return { count: reason.sent };
    case "noReplyAddress":
      return { address: reason.address };
    case "automatedAddress":
    case "newsletterAddress":
      return { address: reason.address, pattern: reason.pattern };
    case "localRule":
      return { name: reason.ruleName };
    default:
      return {};
  }
}

/** A reason in words; a list without a name gets its own sentence. */
export function useReasonText(): (reason: CategoryReason) => string {
  const { t } = useTranslation();
  // The values differ per reason, which the typed `t` cannot follow through a union of keys;
  // tests/comfort-category.test.tsx checks that every reason has its text in both languages.
  const text = t as unknown as (key: string, values?: Record<string, string | number>) => string;
  return (reason) => {
    if (reason.kind === "discussionList" && !reason.listId) return t("comfort.reasons.discussionListUnnamed");
    if (reason.kind === "userRule") return text(`comfort.reasons.userRule.${reason.key.kind}`, reasonValues(reason));
    return text(`comfort.reasons.${reason.kind}`, reasonValues(reason));
  };
}

/**
 * "Why here?" in the reading pane: the category of the message and why, and the user's
 * corrections: "always as …" for the sender, the list or the domain (applied to every message,
 * in the background), and marking it important (the "Priority" block). Corrections always win
 * over the automatic sorting.
 */
export function CategoryPanel({ messageId }: { messageId: MessageId }) {
  const { t } = useTranslation();
  const info = useMessageCategory(messageId);
  const [open, setOpen] = useState(false);
  const panelId = useId();

  if (info.isPending) return null;
  if (info.isError) {
    return (
      <div className="px-6 pb-2">
        <ErrorNotice
          context={t("comfort.category.loadFailed")}
          error={info.error}
          onRetry={() => {
            void info.refetch();
          }}
        />
      </div>
    );
  }
  const decision = info.data.decision;
  const category = decision?.category ?? null;
  const Chevron = open ? ChevronDown : ChevronRight;
  return (
    <div className="flex flex-col gap-2 px-6 pb-2" data-testid="category-panel">
      <div className="flex flex-wrap items-center gap-2 text-13 text-secondary">
        <Tag aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
        <span data-testid="category-current">
          {category ? t("comfort.category.current", { category: t(`comfort.categories.${category}`) }) : t("comfort.category.unsorted")}
        </span>
        <button
          type="button"
          aria-expanded={open}
          aria-controls={panelId}
          onClick={() => {
            setOpen((value) => !value);
          }}
          data-testid="category-why"
          className="flex items-center gap-1 rounded-control px-1 font-medium text-accent hover:underline focus-visible:outline-2 focus-visible:outline-focus-ring"
        >
          <Chevron aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t("comfort.category.why")}
        </button>
      </div>
      {open && (
        <div id={panelId}>
          <CategoryDetails info={info.data} />
        </div>
      )}
    </div>
  );
}

function CategoryDetails({ info }: { info: MessageCategoryInfo }) {
  const { t } = useTranslation();
  const reasonText = useReasonText();
  const keys = senderKeys(info);
  const [keyKind, setKeyKind] = useState<SenderKeyKind | null>(keys[0]?.kind ?? null);
  const key = keys.find((candidate) => candidate.kind === keyKind) ?? keys[0] ?? null;
  const setCategory = useSetSenderCategory();
  const setImportant = useSetSenderImportant();
  const decision = info.decision;
  const ruleKey = decision?.reason.kind === "userRule" ? decision.reason.key : null;
  const importantKey = decision?.important ?? null;

  const keyOptions: ChoiceOption<SenderKeyKind>[] = keys.map((candidate) => ({
    value: candidate.kind,
    label: t(`comfort.keys.${candidate.kind}`, { value: candidate.value }),
  }));

  return (
    <div className="flex flex-col gap-4 rounded-card border border-border bg-surface-raised p-4" data-testid="category-details">
      <p className="text-13 text-primary" data-testid="category-reason">
        {decision ? reasonText(decision.reason) : t("comfort.category.notYet")}
      </p>
      {key ? (
        <>
          {keyOptions.length > 1 && (
            <ChoiceGroup
              label={t("comfort.category.keyLabel")}
              value={key.kind}
              options={keyOptions}
              onChange={setKeyKind}
            />
          )}
          <div className="flex flex-col gap-2">
            <span className="text-13 font-medium text-primary">
              {t("comfort.category.always", { value: key.value })}
            </span>
            <div className="flex flex-wrap gap-2">
              {CATEGORIES.map((candidate) => (
                <Button
                  key={candidate}
                  variant={ruleKey?.kind === key.kind && decision?.category === candidate ? "primary" : "secondary"}
                  disabled={setCategory.isPending}
                  onClick={() => {
                    setCategory.mutate({ key, category: candidate });
                  }}
                  data-testid={`category-always-${candidate}`}
                >
                  {t(`comfort.categories.${candidate}`)}
                </Button>
              ))}
              {ruleKey && (
                <Button
                  variant="quiet"
                  disabled={setCategory.isPending}
                  onClick={() => {
                    setCategory.mutate({ key: ruleKey, category: null });
                  }}
                  data-testid="category-automatic"
                >
                  {t("comfort.category.automatic")}
                </Button>
              )}
            </div>
            <p className="text-12 text-secondary">{t("comfort.category.alwaysHint")}</p>
          </div>
          <Switch
            label={t("comfort.category.important", { value: (importantKey ?? key).value })}
            description={t("comfort.category.importantHint")}
            checked={importantKey !== null}
            disabled={setImportant.isPending}
            onChange={(important) => {
              setImportant.mutate({ key: importantKey ?? key, important });
            }}
            data-testid="category-important"
          />
          {setCategory.isError && <ErrorNotice context={t("comfort.category.saveFailed")} error={setCategory.error} />}
          {setImportant.isError && <ErrorNotice context={t("comfort.category.saveFailed")} error={setImportant.error} />}
        </>
      ) : (
        <p className="text-13 text-secondary">{t("comfort.category.noKey")}</p>
      )}
    </div>
  );
}
