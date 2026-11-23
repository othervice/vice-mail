import { X } from "lucide-react";
import { useEffect, useId, useRef, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { usePaletteStore } from "../../stores/palette";
import { useBoundShortcuts } from "../../stores/shortcuts";
import { useUiStore } from "../../stores/ui";
import { formatShortcut, type ShortcutPlatform } from "../actions/shortcuts";
import { useFocusReturn } from "../palette/CommandPalette";
import { COMMAND_IDS, isActionId, type CommandId } from "./bindings";
import { useCommandLabel } from "./labels";

const MOVING = ["list", "open", "back", "extend"] as const;

/** Keeps Tab and Shift+Tab inside a modal panel. */
export function trapTab(event: KeyboardEvent<HTMLElement>): void {
  if (event.key !== "Tab") return;
  const focusables = [...event.currentTarget.querySelectorAll<HTMLElement>("button:not([disabled]), input, [tabindex='0']")];
  const first = focusables[0];
  const last = focusables[focusables.length - 1];
  if (!first || !last) return;
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

/** "?" (docs/stages/08): all shortcuts at a glance, as they are assigned now. */
export function ShortcutOverview({ platform }: { platform: ShortcutPlatform }) {
  const open = usePaletteStore((state) => state.overview);
  return open ? <OverviewPanel platform={platform} /> : null;
}

function OverviewPanel({ platform }: { platform: ShortcutPlatform }) {
  const { t } = useTranslation();
  const titleId = useId();
  const close = usePaletteStore((state) => state.closeOverview);
  const showSettings = useUiStore((state) => state.showSettings);
  const shortcutsOf = useBoundShortcuts(platform);
  const label = useCommandLabel();
  const closeRef = useRef<HTMLButtonElement>(null);
  useFocusReturn();
  useEffect(() => {
    closeRef.current?.focus();
  }, []);

  const row = (id: CommandId) => {
    const shortcuts = shortcutsOf(id);
    return (
      <div key={id} className="flex items-baseline justify-between gap-4 py-1" data-testid={`overview-${id}`}>
        <dt className="min-w-0 text-13 text-primary">{label(id)}</dt>
        <dd className="shrink-0 text-12 text-secondary">
          {shortcuts.length === 0
            ? t("keyboard.overview.none")
            : shortcuts.map((shortcut, index) => (
                <span key={formatShortcut(shortcut, platform)}>
                  {index > 0 && ` ${t("keyboard.overview.or")} `}
                  <kbd className="rounded-control border border-border px-1.5 font-sans text-primary">{formatShortcut(shortcut, platform)}</kbd>
                </span>
              ))}
        </dd>
      </div>
    );
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      data-testid="shortcut-overview"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          close();
          return;
        }
        trapTab(event);
      }}
      className="fixed top-12 left-1/2 z-50 flex max-h-[calc(100%-4rem)] w-(--ui-overview-width) max-w-[calc(100%-2rem)] -translate-x-1/2 flex-col gap-4 rounded-panel border border-border bg-surface-raised p-6 shadow-panel"
    >
      <div className="flex items-center justify-between gap-2">
        <h2 id={titleId} className="text-16 font-semibold text-primary">
          {t("keyboard.overview.title")}
        </h2>
        <button
          ref={closeRef}
          type="button"
          aria-label={t("keyboard.overview.close")}
          onClick={close}
          className="flex size-8 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
        >
          <X aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
        </button>
      </div>
      <div className="grid min-h-0 grid-cols-1 gap-x-8 gap-y-4 overflow-y-auto md:grid-cols-2">
        <section aria-labelledby={`${titleId}-actions`}>
          <h3 id={`${titleId}-actions`} className="pb-1 text-13 font-semibold text-secondary">
            {t("keyboard.overview.groups.actions")}
          </h3>
          <dl>{COMMAND_IDS.filter(isActionId).map(row)}</dl>
        </section>
        <div className="flex flex-col gap-4">
          <section aria-labelledby={`${titleId}-commands`}>
            <h3 id={`${titleId}-commands`} className="pb-1 text-13 font-semibold text-secondary">
              {t("keyboard.overview.groups.commands")}
            </h3>
            <dl>{COMMAND_IDS.filter((id) => !isActionId(id)).map(row)}</dl>
          </section>
          <section aria-labelledby={`${titleId}-moving`}>
            <h3 id={`${titleId}-moving`} className="pb-1 text-13 font-semibold text-secondary">
              {t("keyboard.overview.groups.moving")}
            </h3>
            <dl>
              {MOVING.map((id) => (
                <div key={id} className="flex items-baseline justify-between gap-4 py-1">
                  <dt className="min-w-0 text-13 text-primary">{t(`keyboard.overview.moving.${id}`)}</dt>
                  <dd className="shrink-0 text-12 text-secondary">{t(`keyboard.overview.moving.${id}Keys`)}</dd>
                </div>
              ))}
            </dl>
          </section>
        </div>
      </div>
      <div>
        <Button
          variant="secondary"
          onClick={() => {
            close();
            showSettings("keyboard");
          }}
        >
          {t("keyboard.overview.change")}
        </Button>
      </div>
    </div>
  );
}
