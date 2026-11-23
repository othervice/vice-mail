import { AppWindow, CloudOff, FileX, Forward, LoaderCircle, Paperclip, Reply, ReplyAll, X } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import { Notice } from "../../components/Notice";
import { Tooltip } from "../../components/Tooltip";
import type { Account, BodyFormat, MessageId, MessageView } from "../../services/ipc";
import { openComposer } from "../../stores/composer";
import { useBoundShortcuts } from "../../stores/shortcuts";
import { useUiStore } from "../../stores/ui";
import { OverflowMenu, type OverflowItem } from "../actions/OverflowMenu";
import type { ActionId } from "../actions/registry";
import { formatShortcut, shortcutPlatform } from "../actions/shortcuts";
import { AskBar, QuickReplies } from "../ai/ReadingAi";
import { useAppInfo } from "../app-info/queries";
import { openMessageInWindow } from "../windows/open";
import { Attachments } from "./Attachments";
import { LinkCard } from "./LinkCard";
import { CategoryPanel } from "../comfort/CategoryPanel";
import { OpenReplyHintBar } from "../ai/OpenReplyHints";
import { MessageExtras } from "../extras/MessageExtras";
import { MessageTools } from "../extras/MessageTools";
import { GmailLabels } from "./GmailLabels";
import { addressName } from "./format";
import { MessageHeader, SubjectTitle } from "./MessageHeader";
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
export function FormatToggle({ shown, onChange }: FormatToggleProps) {
  const { t } = useTranslation();
  const options = [
    { value: "html", label: t("reading.format.html") },
    { value: "text", label: t("reading.format.text") },
  ] as const;
  return (
    <div
      role="group"
      aria-label={t("reading.format.label")}
      className="inline-flex shrink-0 gap-1 rounded-card bg-hover p-1"
      data-testid="format-toggle"
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
          className="h-6 rounded-control px-2 text-12 font-medium text-secondary transition-colors hover:text-primary aria-pressed:bg-surface-raised aria-pressed:text-primary aria-pressed:shadow-row"
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
            // Stage 9: a control, not part of the printed message.
            <div data-print-hide>
              <RemoteContentBar
                blocked={body.blockedRemote}
                sender={sender}
                pending={remote.isPending ? (remote.variables ?? null) : null}
                error={remote.isError ? remote.error : null}
                onLoad={(scope) => {
                  remote.mutate(scope);
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

export function Problems({ view }: { view: Pick<MessageView, "problems"> }) {
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

interface ReplyRowProps {
  messageId: MessageId;
  /** Who a reply goes to, for the button's name ("Reply to Grace Hopper"). */
  sender: string;
}

/**
 * The quiet row under an open message: reply, reply all, forward. The same as the toolbar's
 * actions (and their shortcuts), close to where the reading ends. Replies are local drafts, so they
 * work on read-only accounts too.
 */
function ReplyRow({ messageId, sender }: ReplyRowProps) {
  const { t } = useTranslation();
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const shortcutsOf = useBoundShortcuts(platform);
  const hint = (id: ActionId, label: string) => {
    const first = shortcutsOf(id)[0];
    return first ? t("actions.withShortcut", { label, shortcut: formatShortcut(first, platform) }) : label;
  };
  const buttons = [
    { id: "reply", icon: Reply, name: t("reading.replyRow.reply", { name: sender }), label: t("actions.labels.reply"), kind: "reply" },
    { id: "replyAll", icon: ReplyAll, name: t("reading.replyRow.replyAll"), label: t("actions.labels.replyAll"), kind: "reply_all" },
    { id: "forward", icon: Forward, name: t("reading.replyRow.forward"), label: t("actions.labels.forward"), kind: "forward" },
  ] as const;
  return (
    <div role="group" aria-label={t("reading.replyRow.label")} className="flex items-center gap-1" data-testid="reply-row">
      {buttons.map((button) => (
        <Tooltip key={button.id} label={hint(button.id, button.label)}>
          <button
            type="button"
            aria-label={button.name}
            onClick={() => {
              openComposer({ kind: button.kind, messageId });
            }}
            className="flex size-8 items-center justify-center rounded-control text-secondary transition-colors hover:bg-hover hover:text-primary"
            data-testid={`reply-row-${button.id}`}
          >
            <button.icon aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          </button>
        </Tooltip>
      ))}
    </div>
  );
}

interface CardMenuProps {
  account: Account;
  messageId: MessageId;
  inWindow: boolean;
  draft: boolean;
}

/** The "…" of a message card: what else can be done with this one message. */
function CardMenu({ account, messageId, inWindow, draft }: CardMenuProps) {
  const { t } = useTranslation();
  const items: OverflowItem[] = [
    ...(draft
      ? []
      : [
          { id: "replyAll", label: t("actions.labels.replyAll"), icon: ReplyAll, onSelect: () => { openComposer({ kind: "reply_all", messageId }); } },
          { id: "forwardAsAttachment", label: t("actions.labels.forwardAsAttachment"), icon: Paperclip, onSelect: () => { openComposer({ kind: "forward_as_attachment", messageId }); } },
        ]),
    ...(inWindow
      ? []
      : [{ id: "openInWindow", label: t("actions.labels.openInWindow"), icon: AppWindow, onSelect: () => { openMessageInWindow(account.id, messageId); } }]),
  ].map((item) => ({ ...item, "data-testid": `card-menu-${item.id}` }));
  return <OverflowMenu label={t("reading.card.menu")} items={items} data-testid="card-menu" />;
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
  /**
   * Stage 7: the conversation around the open message. Gets the open message's card and places
   * it among the folded others; the pane keeps its title, scrolling and top bar.
   */
  conversation?: (card: ReactNode) => ReactNode;
  /** The title while the message itself is still loading (the conversation's subject). */
  title?: string;
  /** A draft: nothing to reply to or forward. */
  draft?: boolean;
  /** In a message window of its own: no AI (the panel is in the main window), no "open in window". */
  inWindow?: boolean;
}

/**
 * The reading pane next to the list (docs/DESIGN.md, "Lesebereich"). Shows the message the list
 * selected as a card under its subject: head, notes about damaged or partly stored mail,
 * attachments, blocked external content, the body, and a quiet row to reply (with the AI chips
 * where AI may work on it). Below it the check of a clicked link; at the bottom, while the AI panel
 * is folded, the ask bar. The top bar holds the mail actions (stage 4); marking as read on open
 * is the mail view's job (it knows the setting).
 */
export function ReadingPane({
  account,
  messageId,
  focusRequest,
  toolbar,
  onEscape,
  onClose,
  conversation,
  title,
  draft = false,
  inWindow = false,
}: ReadingPaneProps) {
  const { t } = useTranslation();
  // The chosen format belongs to its message: another message of the conversation starts on "auto".
  const [chosenFormat, setChosenFormat] = useState<{ messageId: MessageId; format: BodyFormat } | null>(null);
  const format: BodyFormat = chosenFormat?.messageId === messageId ? chosenFormat.format : "auto";
  const setFormat = (next: BodyFormat) => {
    setChosenFormat({ messageId, format: next });
  };
  const query = useMessageView(account.id, messageId, format);
  const linkRequest = useUiStore((state) => state.linkRequest);
  const articleRef = useRef<HTMLElement>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  const subjectId = useId();
  const handledFocus = useRef<number | null>(null);
  const view = query.data?.id === messageId ? query.data : undefined;
  const skeleton = useDelayed(query.isPending, SKELETON_DELAY_MS);

  useEffect(() => {
    if (!focusRequest || focusRequest.messageId !== messageId || focusRequest.seq === handledFocus.current) return;
    handledFocus.current = focusRequest.seq;
    articleRef.current?.focus();
  }, [focusRequest, messageId]);

  // In a conversation the open card comes into view when it is far down (or above).
  const loadedId = view?.id ?? null;
  const inConversation = conversation !== undefined;
  useEffect(() => {
    if (!inConversation || loadedId === null) return;
    const scroller = scrollerRef.current;
    const card = cardRef.current;
    if (!scroller || !card) return;
    const delta = card.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
    if (delta < 0 || delta > scroller.clientHeight * 0.6) scroller.scrollTop += delta - 12;
  }, [inConversation, loadedId]);

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
  const sender = view?.header.from.find((address) => address.email.trim() || address.name?.trim());

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

  let card;
  if (query.isError) {
    card = <ErrorNotice context={t("reading.loadFailed")} error={query.error} onRetry={retry} data-testid="reading-error" />;
  } else if (!view) {
    card = (
      <div ref={cardRef} className={CARD}>
        <div className="px-5 py-5">
          <Skeleton label={t("reading.loading")} visible={skeleton} />
        </div>
      </div>
    );
  } else {
    card = (
      <div ref={cardRef} className={CARD} data-testid="message-card">
        <div className="flex flex-col gap-4 px-4 pt-4 pb-2 min-[72rem]:px-5">
          <div className="flex flex-col gap-3">
            <MessageHeader
              header={view.header}
              account={account}
              subjectId={subjectId}
              showSubject={false}
              menu={<CardMenu account={account} messageId={view.id} inWindow={inWindow} draft={draft} />}
              collapsible
            />
            {/* Stage 9: "Why here?" and "always as …" (not printed), as a quiet chip under the head. */}
            <div className="contents" data-print-hide>
              <CategoryPanel messageId={view.id} />
            </div>
          </div>
          {/* Stage 14 (decision 0034): Gmail labels; nothing for other servers. */}
          <GmailLabels account={account} messageId={view.id} />
          <MessageExtras accountId={account.id} messageId={view.id} />
          {/* Stage 13: "maybe still open" (rules in the core, no AI). */}
          <OpenReplyHintBar messageId={view.id} />
          <Problems view={view} />
          <Attachments messageId={view.id} attachments={view.attachments} partial={view.partial} />
          {/* HTML or plain text: a small switch at the body it changes, not in the toolbar. */}
          {canToggle && shownFormat && (
            <div className="-mb-2 flex justify-end" data-print-hide>
              <FormatToggle shown={shownFormat} onChange={setFormat} />
            </div>
          )}
          <Body view={view} account={account} format={format} scrollerRef={scrollerRef} onRetry={retry} />
        </div>
        <div className="flex flex-wrap items-center justify-between gap-2 px-3 pb-3" data-print-hide>
          {draft ? <span /> : <ReplyRow messageId={view.id} sender={(sender && addressName(sender)) ?? t("list.unknownSender")} />}
          {!inWindow && <QuickReplies account={account} messageId={view.id} />}
        </div>
      </div>
    );
  }
  const subject = view?.header.subject ?? title;

  return (
    <section
      aria-label={t("reading.region")}
      onKeyDown={onKeyDown}
      data-testid="reading-pane"
      className="relative flex min-h-0 min-w-0 flex-1 flex-col bg-surface"
    >
      <div className="flex h-12 shrink-0 items-center gap-1 border-b border-border-subtle ps-3 pe-2">
        <div className="flex min-w-0 flex-1 items-center">{toolbar}</div>
        {query.isFetching && view && (
          <LoaderCircle aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
        )}
        {/* Stage 9: save as .eml, print (the print styles show only the article below). */}
        {view && <MessageTools key={view.id} messageId={view.id} />}
        <span aria-hidden className="mx-1 h-5 w-px shrink-0 bg-border" />
        <IconButton icon={X} label={t("reading.close")} onClick={onClose} />
      </div>
      <div ref={scrollerRef} className="min-h-0 flex-1 overflow-y-auto" data-testid="reading-scroller">
        <article
          ref={articleRef}
          tabIndex={-1}
          aria-labelledby={subject !== undefined ? subjectId : undefined}
          aria-label={subject !== undefined ? undefined : t("reading.region")}
          aria-busy={query.isPending || body?.kind === "loading"}
          data-print-root
          className="mx-auto flex w-full max-w-(--ui-reading-column) flex-col gap-4 px-4 pt-5 pb-(--ui-reading-ask-room) focus-visible:-outline-offset-2 min-[72rem]:px-6 min-[72rem]:pt-6"
        >
          {subject !== undefined && <SubjectTitle subject={subject} subjectId={subjectId} />}
          {conversation ? conversation(card) : card}
        </article>
      </div>
      {link && <LinkCard key={link.seq} request={link} onClosed={backToMessage} onReload={retry} />}
      {!inWindow && <AskBar account={account} />}
    </section>
  );
}

/** A message as a card on the reading surface: raised, rounded, a soft shadow instead of a frame. */
const CARD = "flex flex-col rounded-card border border-border-subtle bg-surface-raised shadow-card";
