import { useQuery } from "@tanstack/react-query";
import { FileDown, X } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import { Notice } from "../../components/Notice";
import { sourceIpc, toCoreError, uiIpc, type AccountId, type CoreError, type MessageId } from "../../services/ipc";
import { useLayoutStore } from "../../stores/layout";
import { useAppInfo } from "../app-info/queries";
import { usePlatformAttribute } from "../app-info/usePlatformAttribute";
import { useExportMessage } from "../extras/queries";
import { useUiPreferencesLoad } from "../keyboard/useUiPreferencesSync";
import { useAccentStore } from "../preferences/accent";
import { DEFAULT_PREFERENCES, usePreferences } from "../preferences/queries";
import { usePreferenceEffects } from "../preferences/usePreferenceEffects";
import { formatSize } from "../reading/format";

interface SourceWindowProps {
  accountId: AccountId;
  messageId: MessageId;
}

/**
 * Stage 19 (decision 0055): the root of a source window (`?window=source`). Its capability allows
 * closing itself and nothing else: no core events. Theme, language, accent and text size are read
 * once when it opens.
 */
export function SourceRoot({ accountId, messageId }: SourceWindowProps) {
  const preferences = usePreferences();
  const appInfo = useAppInfo();
  const ui = useUiPreferencesLoad();
  usePreferenceEffects(preferences.data ?? DEFAULT_PREFERENCES);
  usePlatformAttribute(appInfo.data?.platform);
  useEffect(() => {
    if (!ui.data) return;
    useLayoutStore.getState().applyStoredTextSize(ui.data.textSize);
    useAccentStore.getState().applyStored(ui.data.accentTheme);
  }, [ui.data]);
  if (!(preferences.isFetched || preferences.isError) || !(appInfo.isFetched || appInfo.isError)) return null;
  return <SourceWindow accountId={accountId} messageId={messageId} />;
}

/**
 * The message as the server has it, as plain text in a fixed-width font: never interpreted, never
 * markup (React writes it as one text node). Hidden characters arrive as visible symbols from the
 * core. A very large message is cut; "Save as .eml" saves the whole. Escape closes the window.
 */
export function SourceWindow({ accountId, messageId }: SourceWindowProps) {
  const { t, i18n } = useTranslation();
  const headingId = useId();
  const [wrap, setWrap] = useState(true);
  const [closeError, setCloseError] = useState<CoreError | null>(null);
  const query = useQuery({
    queryKey: ["messageSource", accountId, messageId],
    queryFn: () => sourceIpc.getSource(messageId),
    staleTime: Infinity,
    retry: false,
  });
  const exportMessage = useExportMessage();
  const source = query.data;

  const close = () => {
    setCloseError(null);
    uiIpc.closeThisWindow().catch((e: unknown) => {
      setCloseError(toCoreError(e));
    });
  };
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      setCloseError(null);
      uiIpc.closeThisWindow().catch((e: unknown) => {
        setCloseError(toCoreError(e));
      });
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, []);

  const subject = source?.subject.trim();
  return (
    <main aria-labelledby={headingId} className="flex h-full flex-col bg-surface text-primary" data-testid="source-window">
      <header className="flex min-h-12 shrink-0 items-center gap-3 border-b border-border-subtle px-4 py-2">
        <div className="flex min-w-0 flex-1 flex-col">
          <h1 id={headingId} className="text-14 font-semibold text-primary">
            {t("source.title")}
          </h1>
          {source && (
            <p className="truncate text-12 text-secondary">
              {subject || t("list.noSubject")}
              {" · "}
              {t("source.size", { size: formatSize(source.totalBytes, i18n.language) })}
            </p>
          )}
        </div>
        <label className="flex shrink-0 items-center gap-2 text-13 text-secondary">
          <input
            type="checkbox"
            checked={wrap}
            onChange={(e) => {
              setWrap(e.target.checked);
            }}
            data-testid="source-wrap"
          />
          {t("source.wrap")}
        </label>
        <IconButton
          icon={FileDown}
          label={t("source.save")}
          disabled={exportMessage.isPending}
          onClick={() => {
            exportMessage.mutate(messageId);
          }}
        />
        <IconButton icon={X} label={t("source.close")} onClick={close} />
      </header>
      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-auto p-4" data-testid="source-scroller">
        {closeError && <ErrorNotice context={t("source.closeFailed")} error={closeError} />}
        {exportMessage.isSuccess && exportMessage.data && (
          <p role="status" className="text-13 break-all text-secondary select-text" data-testid="source-saved">
            {t("source.saved", { path: exportMessage.data })}
          </p>
        )}
        {exportMessage.isError && <ErrorNotice context={t("source.saveFailed")} error={exportMessage.error} />}
        {query.isError ? (
          <ErrorNotice
            context={t("source.loadFailed")}
            error={query.error}
            onRetry={() => {
              void query.refetch();
            }}
            data-testid="source-error"
          />
        ) : !source ? (
          <p role="status" className="text-13 text-secondary" data-testid="source-loading">
            {t("source.loading")}
          </p>
        ) : (
          <>
            {source.truncated && (
              <Notice
                tone="warning"
                title={t("source.truncatedTitle", {
                  shown: formatSize(source.shownBytes, i18n.language),
                  total: formatSize(source.totalBytes, i18n.language),
                })}
                data-testid="source-truncated"
              >
                <p>{t("source.truncatedBody")}</p>
                <div>
                  <Button
                    variant="secondary"
                    disabled={exportMessage.isPending}
                    onClick={() => {
                      exportMessage.mutate(messageId);
                    }}
                    data-testid="source-save-whole"
                  >
                    {t("source.save")}
                  </Button>
                </div>
              </Notice>
            )}
            <pre
              tabIndex={0}
              aria-label={t("source.title")}
              className={cx(
                "min-w-0 rounded-card border border-border-subtle bg-canvas p-3 font-mono text-12 text-primary select-text",
                wrap ? "break-all whitespace-pre-wrap" : "w-max min-w-full whitespace-pre",
              )}
              data-testid="source-text"
            >
              {source.text}
            </pre>
          </>
        )}
      </div>
    </main>
  );
}
