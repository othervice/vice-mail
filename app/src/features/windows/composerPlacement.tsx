import { AppWindow } from "lucide-react";
import { createContext, useContext } from "react";
import { useTranslation } from "react-i18next";
import { Tooltip } from "../../components/Tooltip";

/**
 * Stage 8 (decision 0025): the composer fills a window of its own instead of lying over the
 * reading pane. The composer window sets this; the composer then takes the whole window and does
 * not offer to open itself in a window again.
 */
export const ComposerInWindow = createContext(false);

export function useComposerInWindow(): boolean {
  return useContext(ComposerInWindow);
}

interface OpenButtonProps {
  disabled: boolean;
  busy: boolean;
  onOpen: () => void;
}

/** "Open in own window" in the composer's header (only in the panel of the main window). */
export function OpenComposerWindowButton({ disabled, busy, onOpen }: OpenButtonProps) {
  const { t } = useTranslation();
  const inWindow = useComposerInWindow();
  if (inWindow) return null;
  return (
    <Tooltip label={t("windows.composer.open")}>
      <button
        type="button"
        aria-label={t("windows.composer.open")}
        aria-busy={busy}
        disabled={disabled || busy}
        onClick={onOpen}
        className="flex size-8 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary disabled:opacity-50"
        data-testid="composer-open-window"
      >
        <AppWindow aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
      </button>
    </Tooltip>
  );
}
