import { useTranslation } from "react-i18next";
import { Wordmark } from "../../components/Brand";
import { Button } from "../../components/Button";
import { PRODUCT_NAME } from "../../config/product";
import { Promises } from "../first-start/Promises";

/**
 * Design pass 23.11.2026: the main area before any account exists (after the guided first start
 * of stage 15 was skipped or done without one). The wordmark from docs/brand, the one next step
 * (add an account) and the promises that hold from the first account on. Calm: no illustration,
 * the brand colour only in the mark and the primary button.
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
        {/* Short facts, not a list of mail: the main area holds no list while there is no account. */}
        <div className="mt-4 w-full">
          <Promises raised />
        </div>
      </div>
    </div>
  );
}
