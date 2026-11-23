import { CloudOff, FileX, LoaderCircle, X } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import { Notice } from "../../components/Notice";
import type { Account, BodyFormat, MessageId, MessageView } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { Attachments } from "./Attachments";
import { LinkCard } from "./LinkCard";
import { CategoryPanel } from "../comfort/CategoryPanel";
import { MessageHeader } from "./MessageHeader";
import { HtmlBody, TextBody } from "./MessageBodyView";
import { useLoadRemoteContent, useMessageView } from "./queries";
import { RemoteContentBar } from "./RemoteContentBar";

/** The list asks the pane to take the focus (Enter or double click on a row). */
export interface FocusRequest {
  messageId: MessageId;
  seq: number;
}

/** A local answer takes milliseconds; a skeleton only for slower ones avoids a flash. */
const SKELETON_DELAY_MS = 200;

function useDelayed(active: boolean, delay: number): boolean {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    if (!active) return undefined;
    const timer = window.setTimeout(() => {
      setShown(true);
    }, delay);
    return () => {
      window.clearTimeout(timer);
      setShown(false);
    };
  }, [active, delay]);
  return active && shown;
}

/** Line widths of the skeleton, like the start of a paragraph. */
const SKELETON_LINES = ["w-full", "w-full", "w-2/3", "w-full", "w-1/2"] as const;

function Skeleton({ label, visible }: { label: string; visible: boolean }) {
  return (
    <div className="flex flex-col gap-3" data-testid="reading-skeleton">
      <p role="status" className="sr-only">
        {label}
      </p>
      {visible &&
        SKELETON_LINES.map((width, i) => (
          <span key={i} aria-hidden className={cx("h-3 rounded-control bg-hover", width)} />
        ))}
    </div>
  );
}

interface FormatToggleProps {
  shown: "html" | "text";
  onChange: (format: BodyFormat) => void;
}

/** HTML or plain text, for mails that have both. Two toggle buttons; the shown one is pressed. */
function FormatToggle({ shown, onChange }: FormatToggleProps) {
  const { t } = useTranslation();
  const options = [
    { value: "html", label: t("reading.format.html") },
    { value: "text", label: t("reading.format.text") },
  ] as const;
  return (
    <div
      role="group"
      aria-label={t("reading.format.label")}
      className="inline-flex gap-1 rounded-card border border-border-strong bg-canvas p-1"
    >
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={shown === option.value}
          onClick={() => {
            onChange(option.value);
          }}
          data-testid={`format-${option.value}`}
          className="h-6 rounded-control px-2 text-12 font-medium text-secondary transition-colors hover:bg-hover hover:text-primary aria-pressed:bg-accent aria-pressed:text-on-accent"
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

interface BodyProps {
  view: MessageView;
  account: Account;
  format: BodyFormat;
  scrollerRef: RefObject<HTMLDivElement | null>;
  onRetry: () => void;
}

/** The body in its state: rendered (HTML or text), loading, or not available with the reason. */
function Body({ view, account, format, scrollerRef, onRetry }: BodyProps) {
  const { t } = useTranslation();
  const remote = useLoadRemoteContent(account.id, view.id, format);
  const subject = view.header.subject.trim();
  const accountName = account.displayName || account.email;
  const { body } = view;

  switch (body.kind) {
    case "html": {
      const sender = view.header.from.find((address) => address.email.trim())?.email.trim() ?? null;
      return (
        <>
          {body.blockedRemote > 0 && !body.remoteLoaded && (
            <RemoteContentBar
              blocked={body.blockedRemote}
              sender={sender}
              pending={remote.isPending ? (remote.variables ?? null) : null}
              error={remote.isError ? remote.error : null}
              onLoad={(scope) => {
                remote.mutate(scope);
              }}
            />
          )}
          <HtmlBody
            srcdoc={body.srcdoc}
            title={subject ? t("reading.frameTitle", { subject }) : t("reading.frameTitleNoSubject")}
            scrollerRef={scrollerRef}
          />
        </>
      );
    }
    case "text":
      return <TextBody segments={body.segments} renderToken={body.renderToken} label={t("reading.textBody")} />;
    case "loading":
      return <LoadingBody />;
    case "unavailable":
      if (body.reason === "fetchFailed") {
        return body.error ? (
          <ErrorNotice context={t("reading.unavailable.fetchFailed")} error={body.error} onRetry={onRetry} data-testid="reading-fetch-failed" />
        ) : (
          <Unavailable icon="gone" title={t("reading.unavailable.fetchFailed")} body={t("errors.codes.internal.hint")} onRetry={onRetry} />
        );
      }
      if (body.reason === "offline") {
        return (
          <Unavailable
            icon="offline"
            title={t("reading.unavailable.offline.title")}
            body={t("reading.unavailable.offline.body", { account: accountName })}
            onRetry={onRetry}
          />
        );
      }
      return <Unavailable icon="gone" title={t("reading.unavailable.notOnServer.title")} body={t("reading.unavailable.notOnServer.body")} />;
  }
}

function LoadingBody() {
  const { t } = useTranslation();
  return <Skeleton label={t("reading.loading")} visible />;
}

interface UnavailableProps {
  icon: "offline" | "gone";
  title: string;
  body: string;
  onRetry?: () => void;
}

function Unavailable({ icon, title, body, onRetry }: UnavailableProps) {
  const { t } = useTranslation();
  const Icon = icon === "offline" ? CloudOff : FileX;
  return (
    <div className="flex flex-col items-start gap-3 rounded-card border border-border bg-canvas p-4" data-testid="reading-unavailable">
      <div className="flex gap-3">
        <Icon aria-hidden className="size-(--ui-icon) shrink-0 text-secondary" strokeWidth={1.75} />
        <div className="flex flex-col gap-1">
          <p className="font-semibold text-primary">{title}</p>
          <p className="text-secondary">{body}</p>
        </div>
      </div>
      {onRetry && (
        <div className="ps-8">
          <Button variant="secondary" onClick={onRetry}>
            {t("errors.retry")}
          </Button>
        </div>
      )}
    </div>
  );
}

function Problems({ view }: { view: MessageView }) {
  const { t } = useTranslation();
  if (view.problems.length === 0) return null;
  return (
    <Notice tone="warning" title={t("reading.problems.title")} data-testid="reading-problems">
      <ul className="flex list-disc flex-col gap-1 ps-4">
        {view.problems.map((problem) => (
          <li key={problem}>{t(`reading.problems.${problem}`)}</li>
        ))}
      </ul>
    </Notice>
  );
}

interface ReadingPaneProps {
  account: Account;
  messageId: MessageId;
  focusRequest: FocusRequest | null;
  /** The mail actions for this message (archive, delete, …), left in the pane's top bar. */
  toolbar?: ReactNode;
  /** Escape: back to the list, the message stays open. */
  onEscape: () => void;
  /** Closes the pane (the list gives up its selection). */
  onClose: () => void;
}

/**
 * The reading pane next to the list (docs/DESIGN.md, "Lesebereich"). Shows the message the list
 * selected: head, notes about damaged or partly stored mail, attachments, blocked external
 * content and the body, and below it the check of a clicked link. The top bar holds the mail
 * actions (stage 4); marking as read on open is the mail view's job (it knows the setting).
 */
export function ReadingPane({ account, messageId, focusRequest, toolbar, onEscape, onClose }: ReadingPaneProps) {
  const { t } = useTranslation();
  const [format, setFormat] = useState<BodyFormat>("auto");
  const query = useMessageView(account.id, messageId, format);
  const linkRequest = useUiStore((state) => state.linkRequest);
  const articleRef = useRef<HTMLElement>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const subjectId = useId();
  const handledFocus = useRef<number | null>(null);
  const view = query.data;
  const skeleton = useDelayed(query.isPending, SKELETON_DELAY_MS);

  useEffect(() => {
    if (!focusRequest || focusRequest.messageId !== messageId || focusRequest.seq === handledFocus.current) return;
    handledFocus.current = focusRequest.seq;
    articleRef.current?.focus();
  }, [focusRequest, messageId]);

  // Not on this computer and the account was offline: ask again once it is connected.
  const { refetch } = query;
  const waitsOffline = view?.body.kind === "unavailable" && view.body.reason === "offline";
  const connected = account.status.state === "idle" || account.status.state === "syncing";
  useEffect(() => {
    if (waitsOffline && connected) void refetch();
  }, [waitsOffline, connected, refetch]);

  const body = view?.body;
  const renderToken = body?.kind === "html" || body?.kind === "text" ? body.renderToken : null;
  const shownFormat = body?.kind === "html" || body?.kind === "text" ? body.kind : null;
  const canToggle = view !== undefined && view.formats.includes("html") && view.formats.includes("text");
  const link = linkRequest && renderToken !== null && linkRequest.renderToken === renderToken ? linkRequest : null;

  const retry = () => {
    void refetch();
  };
  const backToMessage = () => {
    articleRef.current?.focus({ preventScroll: true });
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    onEscape();
  };

  let content;
  if (query.isError) {
    content = (
      <ErrorNotice context={t("reading.loadFailed")} error={query.error} onRetry={retry} data-testid="reading-error" />
    );
  } else if (!view) {
    content = <Skeleton label={t("reading.loading")} visible={skeleton} />;
  } else {
    content = (
      <>
        <MessageHeader header={view.header} account={account} subjectId={subjectId} />
        {/* Stage 9: "Why here?" and "always as …". */}
        <CategoryPanel messageId={view.id} />
        <Problems view={view} />
        <Attachments messageId={view.id} attachments={view.attachments} partial={view.partial} />
        <Body view={view} account={account} format={format} scrollerRef={scrollerRef} onRetry={retry} />
      </>
    );
  }

  return (
    <section
      aria-label={t("reading.region")}
      onKeyDown={onKeyDown}
      data-testid="reading-pane"
      className="flex min-h-0 min-w-0 flex-1 flex-col bg-surface"
    >
      <div className="flex h-12 shrink-0 items-center justify-end gap-2 border-b border-border px-4">
        <div className="me-auto flex min-w-0 items-center">{toolbar}</div>
        {query.isFetching && view && (
          <LoaderCircle aria-hidden className="size-(--ui-icon-small) text-tertiary" strokeWidth={1.75} />
        )}
        {canToggle && shownFormat && <FormatToggle shown={shownFormat} onChange={setFormat} />}
        <IconButton icon={X} label={t("reading.close")} onClick={onClose} />
      </div>
      <div ref={scrollerRef} className="min-h-0 flex-1 overflow-y-auto" data-testid="reading-scroller">
        <article
          ref={articleRef}
          tabIndex={-1}
          aria-labelledby={view ? subjectId : undefined}
          aria-label={view ? undefined : t("reading.region")}
          aria-busy={query.isPending || body?.kind === "loading"}
          className="flex flex-col gap-4 px-6 py-6 outline-none focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus-ring"
        >
          {content}
        </article>
      </div>
      {link && <LinkCard key={link.seq} request={link} onClosed={backToMessage} onReload={retry} />}
    </section>
  );
}
