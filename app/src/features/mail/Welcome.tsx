import { KeyRound, ShieldCheck, Waypoints, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Wordmark } from "../../components/Brand";
import { Button } from "../../components/Button";
import { PRODUCT_NAME } from "../../config/product";

const POINTS: readonly { key: "readOnly" | "local" | "keychain"; icon: LucideIcon }[] = [
  { key: "readOnly", icon: ShieldCheck },
  { key: "local", icon: Waypoints },
  { key: "keychain", icon: KeyRound },
];

/**
 * Design pass 23.11.2026: the first start, before any account exists. The wordmark from
 * docs/brand, the one next step (add an account) and three facts that hold from the first
 * account on. Calm: no illustration, the brand colour only in the mark and the primary button.
 */
export function Welcome({ onAdd }: { onAdd: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center overflow-y-auto px-8 py-10" data-testid="welcome">
      <div className="flex w-full max-w-(--ui-form-max-width) flex-col items-center gap-6 text-center">
        <Wordmark aria-label={PRODUCT_NAME} className="h-(--ui-wordmark-height) w-auto max-w-full text-brand-ink" />
        <div className="flex flex-col items-center gap-2">
          <h2 className="text-20 font-semibold text-primary">{t("mail.noAccount.title")}</h2>
          <p className="text-14 text-secondary">{t("mail.noAccount.body")}</p>
        </div>
        <Button onClick={onAdd} data-testid="welcome-add-account">
          {t("mail.noAccount.action")}
        </Button>
        {/* Three short facts, not a list of mail: the main area holds no list while there is no account. */}
        <div className="mt-4 flex w-full flex-col divide-y divide-border-subtle rounded-card border border-border-subtle bg-surface-raised text-start shadow-card">
          {POINTS.map(({ key, icon: Icon }) => (
            <div key={key} className="flex items-start gap-3 px-4 py-3">
              <span className="flex size-8 shrink-0 items-center justify-center rounded-row bg-accent-soft text-accent">
                <Icon aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
              </span>
              <p className="pt-1 text-13 text-secondary">{t(`mail.noAccount.points.${key}`)}</p>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
