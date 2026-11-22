import { Settings, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cx } from "../components/cx";
import { Tooltip } from "../components/Tooltip";
import { useWindowDragRegion } from "../features/app-info/useWindowDragRegion";
import { MAIL_VIEW_ICONS } from "../features/mail/views";
import { MAIL_VIEW_IDS, useUiStore } from "../stores/ui";

interface RailButtonProps {
  icon: LucideIcon;
  label: string;
  active: boolean;
  onClick: () => void;
  /** Stable hook for the desktop tests (labels change with the language). */
  "data-testid": string;
}

function RailButton({ icon: Icon, label, active, onClick, "data-testid": testId }: RailButtonProps) {
  // Active: accent icon on a soft fill plus a bar at the rail edge, so it is not colour alone.
  const className = cx(
    "relative flex size-(--ui-rail-item) items-center justify-center rounded-card transition-colors",
    active
      ? "bg-accent-soft text-accent before:absolute before:inset-y-2 before:-left-2 before:w-1 before:rounded-control before:bg-accent"
      : "text-secondary hover:bg-hover hover:text-primary",
  );
  return (
    <Tooltip label={label} side="right">
      <button
        type="button"
        aria-label={label}
        aria-current={active ? "page" : undefined}
        onClick={onClick}
        className={className}
        data-testid={testId}
      >
        <Icon aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
      </button>
    </Tooltip>
  );
}

/** The 56 px icon rail: mail views on top, account switcher and settings at the bottom. */
export function IconRail() {
  const { t } = useTranslation();
  const view = useUiStore((state) => state.view);
  const showMailView = useUiStore((state) => state.showMailView);
  const showSettings = useUiStore((state) => state.showSettings);
  const dragRegion = useWindowDragRegion();

  return (
    <nav
      aria-label={t("nav.label")}
      className="flex h-full w-(--ui-rail-width) shrink-0 flex-col items-center border-r border-border bg-rail"
    >
      {/* Room for the window buttons that macOS draws over this corner. */}
      <div aria-hidden data-tauri-drag-region={dragRegion ? "deep" : undefined} className="h-(--ui-titlebar-inset) w-full shrink-0" />
      <ul className="flex min-h-0 w-full flex-1 flex-col items-center gap-1 overflow-y-auto py-3">
        {MAIL_VIEW_IDS.map((id) => (
          <li key={id}>
            <RailButton
              icon={MAIL_VIEW_ICONS[id]}
              label={t(`nav.${id}`)}
              active={view.kind === "mail" && view.mailView === id}
              onClick={() => {
                showMailView(id);
              }}
              data-testid={`rail-${id}`}
            />
          </li>
        ))}
      </ul>
      <div className="flex shrink-0 flex-col items-center gap-2 py-3">
        {/* Account switcher: one entry per account with its colour and name. Empty without accounts. */}
        <ul aria-label={t("nav.accounts")} className="flex flex-col items-center gap-1" />
        <RailButton
          icon={Settings}
          label={t("nav.settings")}
          active={view.kind === "settings"}
          onClick={() => {
            showSettings();
          }}
          data-testid="rail-settings"
        />
      </div>
    </nav>
  );
}
