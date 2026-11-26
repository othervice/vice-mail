import { FileDown, Printer } from "lucide-react";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import type { MessageId } from "../../services/ipc";
import { useAppInfo } from "../app-info/queries";
import { formatShortcut, matchesShortcut, shortcutPlatform, type Shortcut } from "../actions/shortcuts";
import { usePrintRequest } from "../../stores/printRequest";
import { useExportMessage, usePrintWindow } from "./queries";
import "./print.css";

/** Cmd+P on macOS, Ctrl+P on Windows: what every app uses for printing. */
export const PRINT_SHORTCUT: Shortcut = { key: "p", mod: true };

interface MessageToolsProps {
  messageId: MessageId;
  /** Stage 21: runs before printing (the reading pane switches an adapted mail to its original colours). */
  beforePrint?: () => Promise<void>;
  /**
   * Stage 21: the message is shown (head and body), so a print asked for from the context menu
   * (`usePrintRequest`) can start.
   */
  shown?: boolean;
  /** Stage 23: the quiet 28 px buttons of the reading pane's top line (main window). */
  small?: boolean;
}

/**
 * Export and print for the message in the reading pane (stage 9). "Save as .eml" writes the raw
 * message where the user picks in the system's dialog; "Print" opens the system's print dialog
 * (which also saves a PDF) while the print styles show only the cleaned message. Cmd/Ctrl+P
 * prints the message too, instead of the whole window, while a message is open. The reading pane
 * keys it by message, so a result never stays with the next message.
 */
export function MessageTools({ messageId, beforePrint, shown = false, small = false }: MessageToolsProps) {
  const { t } = useTranslation();
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const exportMessage = useExportMessage();
  const print = usePrintWindow(beforePrint);
  const { mutate: printMessage } = print;
  const printAsked = usePrintRequest((state) => state.messageId === messageId);
  const clearPrint = usePrintRequest((state) => state.clear);

  // Stage 21 (decision 0064): "Print" from a row's context menu, once this message is shown.
  useEffect(() => {
    if (!printAsked || !shown) return;
    clearPrint();
    printMessage();
  }, [printAsked, shown, clearPrint, printMessage]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || !matchesShortcut(event, PRINT_SHORTCUT, platform)) return;
      event.preventDefault();
      printMessage();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [platform, printMessage]);

  const savedPath = exportMessage.isSuccess ? exportMessage.data : null;
  const dismiss = () => {
    exportMessage.reset();
    print.reset();
  };
  const error = exportMessage.isError ? { context: t("extras.tools.exportFailed"), error: exportMessage.error } : print.isError ? { context: t("extras.tools.printFailed"), error: print.error } : null;

  return (
    <div role="group" aria-label={t("extras.tools.label")} className="relative flex items-center gap-1" data-testid="message-tools">
      <IconButton
        icon={FileDown}
        small={small}
        label={t("extras.tools.export")}
        disabled={exportMessage.isPending}
        onClick={() => {
          exportMessage.mutate(messageId);
        }}
      />
      <IconButton
        icon={Printer}
        small={small}
        label={t("extras.tools.printWithShortcut", { shortcut: formatShortcut(PRINT_SHORTCUT, platform) })}
        disabled={print.isPending}
        onClick={() => {
          print.mutate();
        }}
      />
      {(savedPath || error) && (
        <div className="absolute end-0 top-full z-20 mt-2 flex w-(--ui-notice-width) max-w-(--ui-notice-width) flex-col gap-2 rounded-card bg-surface-raised shadow-floating">
          {savedPath && (
            <div role="status" className="flex items-start gap-3 rounded-card border border-border px-4 py-3" data-testid="export-saved">
              <p className="min-w-0 flex-1 text-13 break-all text-primary select-text">{t("extras.tools.exported", { path: savedPath })}</p>
              <Button variant="quiet" onClick={dismiss}>
                {t("extras.tools.dismiss")}
              </Button>
            </div>
          )}
          {error && (
            <ErrorNotice
              context={error.context}
              error={error.error}
              actions={
                <Button variant="secondary" onClick={dismiss}>
                  {t("extras.tools.dismiss")}
                </Button>
              }
              data-testid="tools-error"
            />
          )}
        </div>
      )}
    </div>
  );
}
