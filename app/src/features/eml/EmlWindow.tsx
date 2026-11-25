import { FileX, LoaderCircle, X } from "lucide-react";
import { useEffect, useId, useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import { Notice } from "../../components/Notice";
import type { BodyFormat, CoreError, EmlFileId, EmlMessageView } from "../../services/ipc";
import { openComposer, useComposerStore } from "../../stores/composer";
import { useMailtoStore } from "../../stores/mailto";
import { useUiStore } from "../../stores/ui";
import { Notices } from "../actions/Notices";
import { useSelectedAccount } from "../accounts/selection";
import { ComposerHost } from "../composer/Composer";
import { SendNotices } from "../composer/SendNotices";
import { composerSourceFor } from "../extras/mailto";
import { Attachments } from "../reading/Attachments";
import { LinkCard } from "../reading/LinkCard";
import { HtmlBody, TextBody } from "../reading/MessageBodyView";
import { MessageHeader } from "../reading/MessageHeader";
import { FormatToggle, Problems } from "../reading/ReadingPane";
import { RemoteContentBar } from "../reading/RemoteContentBar";
import { TrackerNotice } from "../reading/TrackerNotice";
import { useCloseThisWindow } from "../windows/MessageWindow";
import { useLoadEmlRemoteContent, useEmlView } from "./queries";

/**
 * A `mailto:` link in the file, confirmed in the link card, becomes a new draft of the selected
 * account here (the main window's queue does not reach this window). Nothing is sent.
 */
function useMailtoDrafts(): void {
  const pending = useMailtoStore((state) => state.pending);
  const composing = useComposerStore((state) => state.session !== null);
  const { account } = useSelectedAccount();
  useEffect(() => {
    if (!account || composing || pending.length === 0) return;
    const next = useMailtoStore.getState().shift();
    if (next) openComposer(composerSourceFor(next.request, account.id));
  }, [account, composing, pending]);
}

interface BodyProps {
  view: EmlMessageView;
  format: BodyFormat;
  scrollerRef: RefObject<HTMLDivElement | null>;
}

/** The body as the reading pane shows it: sanitised HTML in the sandboxed frame, or text. */
function EmlBody({ view, format, scrollerRef }: BodyProps) {
  const { t } = useTranslation();
  const remote = useLoadEmlRemoteContent(view.fileId, format);
  const subject = view.header.subject.trim();
  const { body } = view;
  switch (body.kind) {
    case "html":
      return (
        <>
          {/* Stage 20 (decision 0058): the trackers the core blocked, with what that means. */}
          <TrackerNotice trackers={body.trackers} />
          {body.blockedRemote > 0 && !body.remoteLoaded && (
            <div data-print-hide>
              {/* No "always for this sender": the file belongs to no account. */}
              <RemoteContentBar
                blocked={body.blockedRemote}
                sender={null}
                pending={remote.isPending ? "once" : null}
                error={remote.isError ? remote.error : null}
                onLoad={() => {
                  remote.mutate();
                }}
              />
            </div>
          )}
          <HtmlBody
            srcdoc={body.srcdoc}
            title={subject ? t("reading.frameTitle", { subject }) : t("reading.frameTitleNoSubject")}
            scrollerRef={scrollerRef}
          />
        </>
      );
    case "text":
      return <TextBody segments={body.segments} renderToken={body.renderToken} label={t("reading.textBody")} />;
    // A file is read whole: it is never loading or missing.
    case "loading":
    case "unavailable":
      return null;
  }
}

interface EmlWindowProps {
  fileId: EmlFileId;
  eventsError: CoreError | null;
}

/**
 * Stage 9 (decision 0027): an `.eml` file in a window of its own, read-only and outside every
 * account. The same reading pipeline as stored mail (sanitised HTML in the sandboxed frame,
 * external content blocked, links checked by the core, attachments saved or opened by the core);
 * no mail actions, because the message lies in no folder. A file the core refused shows why.
 */
export function EmlWindow({ fileId, eventsError }: EmlWindowProps) {
  const { t } = useTranslation();
  const [format, setFormat] = useState<BodyFormat>("auto");
  const query = useEmlView(fileId, format);
  const linkRequest = useUiStore((state) => state.linkRequest);
  const closing = useCloseThisWindow();
  const articleRef = useRef<HTMLElement>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const subjectId = useId();
  useMailtoDrafts();
  // Drafts of the last session are offered by the main window, not here.
  useEffect(() => {
    useComposerStore.getState().finishRestore();
  }, []);

  const view = query.data;
  const body = view?.body;
  const renderToken = body?.kind === "html" || body?.kind === "text" ? body.renderToken : null;
  const shownFormat = body?.kind === "html" || body?.kind === "text" ? body.kind : null;
  const canToggle = view !== undefined && view.formats.includes("html") && view.formats.includes("text");
  const link = linkRequest && renderToken !== null && linkRequest.renderToken === renderToken ? linkRequest : null;
  const retry = () => {
    void query.refetch();
  };

  let content;
  if (query.isError && query.error.code === "not_found") {
    content = <EmptyState icon={FileX} title={t("eml.gone.title")} body={t("eml.gone.body")} />;
  } else if (query.isError) {
    content = <ErrorNotice context={t("eml.loadFailed")} error={query.error} onRetry={retry} data-testid="eml-error" />;
  } else if (!view) {
    content = (
      <p role="status" className="text-13 text-secondary" data-testid="eml-loading">
        {t("reading.loading")}
      </p>
    );
  } else {
    content = (
      <>
        <Notice title={t("eml.readOnly.title")} data-testid="eml-read-only">
          <p>{t("eml.readOnly.body")}</p>
        </Notice>
        <MessageHeader header={view.header} fileName={view.fileName} subjectId={subjectId} />
        <Problems view={view} />
        <Attachments fileId={view.fileId} attachments={view.attachments} partial={false} />
        <EmlBody view={view} format={format} scrollerRef={scrollerRef} />
      </>
    );
  }

  return (
    <div className="flex h-full bg-canvas text-primary" data-window="eml">
      <main className="relative flex min-w-0 flex-1 flex-col bg-surface" aria-label={t("eml.region")}>
        {eventsError && (
          <div className="px-6 pt-4">
            <ErrorNotice context={t("errors.context.events")} error={eventsError} />
          </div>
        )}
        {closing.error && (
          <div className="px-6 pt-4">
            <ErrorNotice context={t("windows.closeFailed")} error={closing.error} data-testid="window-close-failed" />
          </div>
        )}
        <section aria-label={t("reading.region")} data-testid="eml-pane" className="flex min-h-0 min-w-0 flex-1 flex-col">
          <div className="flex h-12 shrink-0 items-center justify-end gap-2 border-b border-border-subtle px-4">
            <p className="me-auto truncate text-13 text-secondary" data-testid="eml-file-name">
              {view?.fileName ?? ""}
            </p>
            {query.isFetching && view && (
              <LoaderCircle aria-hidden className="size-(--ui-icon-small) text-tertiary" strokeWidth={1.75} />
            )}
            {canToggle && shownFormat && <FormatToggle shown={shownFormat} onChange={setFormat} />}
            <IconButton icon={X} label={t("eml.close")} onClick={closing.close} />
          </div>
          <div ref={scrollerRef} className="min-h-0 flex-1 overflow-y-auto" data-testid="eml-scroller">
            <article
              ref={articleRef}
              tabIndex={-1}
              aria-labelledby={view ? subjectId : undefined}
              aria-label={view ? undefined : t("eml.region")}
              aria-busy={query.isPending}
              data-print-root
              className="mx-auto flex w-full max-w-(--ui-reading-column) flex-col gap-4 px-6 py-8 focus-visible:-outline-offset-2"
            >
              {content}
            </article>
          </div>
          {link && (
            <LinkCard
              key={link.seq}
              request={link}
              onClosed={() => {
                articleRef.current?.focus({ preventScroll: true });
              }}
              onReload={retry}
            />
          )}
        </section>
        <ComposerHost />
      </main>
      <Notices />
      <SendNotices />
    </div>
  );
}
