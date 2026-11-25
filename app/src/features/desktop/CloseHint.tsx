import { Info } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { ErrorNotice } from "../../components/ErrorNotice";
import { PRODUCT_NAME } from "../../config/product";
import { desktopIpc, toCoreError, type CoreError } from "../../services/ipc";
import { useAppInfo } from "../app-info/queries";

/**
 * Stage 21 (decision 0069): the first time the main window is closed with "keep running" on,
 * the shell keeps it open and asks this page to explain once how the app goes on in the
 * background and how to quit. "Keep running" closes the window into the background, "Quit"
 * ends the app, Cancel (or Escape) leaves the window open. Either answer is remembered by the
 * core, so the note never comes again on this device.
 */
export function CloseHintHost() {
  const { t } = useTranslation();
  const product = PRODUCT_NAME;
  const macos = useAppInfo().data?.platform === "macos";
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<CoreError | null>(null);

  useEffect(() => {
    let active = true;
    let stop: (() => void) | undefined;
    desktopIpc
      .onCloseToBackgroundAsked(() => {
        setError(null);
        setOpen(true);
      })
      .then(
        (unsubscribe) => {
          if (active) stop = unsubscribe;
          else unsubscribe();
        },
        () => undefined,
      );
    return () => {
      active = false;
      stop?.();
    };
  }, []);

  const answer = (quit: boolean) => {
    setOpen(false);
    desktopIpc.closeMainWindow(quit).catch((e: unknown) => {
      setError(toCoreError(e));
    });
  };

  return (
    <>
      {open && (
        <ConfirmDialog
          tone="neutral"
          icon={Info}
          title={t("desktop.closeHint.title", { product })}
          body={`${t(macos ? "desktop.closeHint.macos" : "desktop.closeHint.windows", { product })} ${t("desktop.closeHint.settings")}`}
          confirmLabel={t("desktop.closeHint.keepRunning")}
          cancelLabel={t("desktop.closeHint.cancel")}
          secondary={{
            label: t("desktop.closeHint.quit", { product }),
            onClick: () => {
              answer(true);
            },
          }}
          onConfirm={() => {
            answer(false);
          }}
          onCancel={() => {
            setOpen(false);
          }}
          data-testid="close-hint"
        />
      )}
      {error && (
        <div className="pointer-events-none fixed end-6 top-(--ui-header-height) z-30 mt-2 w-(--ui-notice-width) max-w-full">
          <div className="pointer-events-auto rounded-card bg-surface-raised shadow-floating">
            <ErrorNotice context={t("desktop.closeHint.failed")} error={error} />
          </div>
        </div>
      )}
    </>
  );
}
