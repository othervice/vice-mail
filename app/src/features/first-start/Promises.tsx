import { EyeOff, KeyRound, ShieldCheck, Waypoints, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";

type PromiseKey = "readOnly" | "local" | "telemetry" | "keychain";

const POINTS: readonly { key: PromiseKey; icon: LucideIcon }[] = [
  { key: "readOnly", icon: ShieldCheck },
  { key: "local", icon: Waypoints },
  { key: "telemetry", icon: EyeOff },
  { key: "keychain", icon: KeyRound },
];

/**
 * The four promises of the first start (docs/DESIGN.md, "Erster Start"): new accounts only read,
 * no cloud service in between, no telemetry, passwords in the system keychain. Shown on the
 * welcome step of the guided first start and in the main area while there is no account.
 * Plain rows, not list items: the main area holds no list while there is no account.
 */
export function Promises({ raised = false, "aria-label": label }: { raised?: boolean; "aria-label"?: string }) {
  const { t } = useTranslation();
  return (
    <div
      role={label ? "group" : undefined}
      aria-label={label}
      // Raised (the main area without an account): a card with hairlines, as everywhere. Plain
      // (inside the first start's card, itself raised): the regular border, so the group still
      // shows on the raised surface of the dark theme.
      className={cx(
        "flex w-full flex-col divide-y rounded-card border text-start",
        raised ? "divide-border-subtle border-border-subtle bg-surface-raised shadow-card" : "divide-border border-border",
      )}
      data-testid="promises"
    >
      {POINTS.map(({ key, icon: Icon }) => (
        <div key={key} className="flex items-start gap-3 px-4 py-3">
          <span className="flex size-8 shrink-0 items-center justify-center rounded-tile bg-accent-soft text-accent">
            <Icon aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          </span>
          <p className="pt-1 text-13 text-secondary">{t(`mail.noAccount.points.${key}`)}</p>
        </div>
      ))}
    </div>
  );
}
