import { AppWindow, CloudOff, FileX, Forward, LoaderCircle, Paperclip, Reply, ReplyAll, X } from "lucide-react";
// Stage 19 (decision 0055): source, all headers, redirect, edit as new.
import { CopyPlus, CornerUpRight, FileCode, ListTree } from "lucide-react";
import { useNoticeStore } from "../../stores/notices";
import { useRedirectStore } from "../../stores/redirect";
import { openSourceInWindow } from "../windows/open";
import { AllHeaders } from "./AllHeaders";
import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import { Notice } from "../../components/Notice";
import { Tooltip } from "../../components/Tooltip";
import type { Account, BodyFormat, MailScheme, MessageId, MessageView } from "../../services/ipc";
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
import { ColorsToggle, HtmlBody, mailScheme, TextBody, whenFrameShows } from "./MessageBodyView";
import { useAppliedTheme } from "../preferences/theme";
import { useLoadRemoteContent, useMessageView } from "./queries";
import { RemoteContentBar } from "./RemoteContentBar";
import { TrackerNotice } from "./TrackerNotice";
import { BlockedNote } from "../organize/BlockedNote";
// Stage 16: find in the open message.
import { FindBar, useFindShortcut } from "./FindBar";
// Stage 16: read receipts (RFC 8098) and the inline quick reply.
import { ReadReceipts } from "./ReadReceipts";
import { QuickReply } from "../composer/QuickReply";

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
  /** Stage 21: the scheme the view was asked in (loading external content keeps it). */
  scheme: MailScheme;
  scrollerRef: RefObject<HTMLDivElement | null>;
  onRetry: () => void;
}

/** The body in its state: rendered (HTML or text), loading, or not available with the reason. */
function Body({ view, account, format, scheme, scrollerRef, onRetry }: BodyProps) {
  const { t } = useTranslation();
  const remote = useLoadRemoteContent(account.id, view.id, format, scheme);
  const subject = view.header.subject.trim();
  const accountName = account.displayName || account.email;
  const { body } = view;

  switch (body.kind) {
    case "html": {
      const sender = view.header.from.find((address) => address.email.trim())?.email.trim() ?? null;
      return (
        <>
          {/* Stage 20 (decision 0058): the trackers the core blocked, with what that means. */}
          <TrackerNotice trackers={body.trackers} />
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
            colors={body.colors}
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
  /** Stage 19: "all headers" is shown under the head. */
  allHeaders: boolean;
  onToggleAllHeaders: () => void;
}

/** The "…" of a message card: what else can be done with this one message. */
function CardMenu({ account, messageId, inWindow, draft, allHeaders, onToggleAllHeaders }: CardMenuProps) {
  const { t } = useTranslation();
  const openRedirect = useRedirectStore((state) => state.open);
  const pushNotice = useNoticeStore((state) => state.push);
  const items: OverflowItem[] = [
    ...(draft
      ? []
      : [
          { id: "replyAll", label: t("actions.labels.replyAll"), icon: ReplyAll, onSelect: () => { openComposer({ kind: "reply_all", messageId }); } },
          { id: "forwardAsAttachment", label: t("actions.labels.forwardAsAttachment"), icon: Paperclip, onSelect: () => { openComposer({ kind: "forward_as_attachment", messageId }); } },
          // Stage 19: a send, so it says why it does nothing on a read-only account.
          {
            id: "redirect",
            label: account.writesEnabled ? t("actions.labels.redirect") : t("actions.readOnly.tooltip", { label: t("actions.labels.redirect") }),
            icon: CornerUpRight,
            inert: !account.writesEnabled,
            onSelect: () => {
              if (account.writesEnabled) openRedirect({ accountId: account.id, messageId });
              else pushNotice({ kind: "readOnly", accountId: account.id });
            },
          },
        ]),
    { id: "editAsNew", label: t("actions.labels.editAsNew"), icon: CopyPlus, onSelect: () => { openComposer({ kind: "edit_as_new", messageId }); } },
    { id: "allHeaders", label: allHeaders ? t("source.allHeaders.hide") : t("source.allHeaders.show"), icon: ListTree, onSelect: onToggleAllHeaders },
    { id: "viewSource", label: t("actions.labels.viewSource"), icon: FileCode, onSelect: () => { openSourceInWindow(account.id, messageId); } },
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
  /** The title while the message itself is still loading (the conversation's subject). */
  title?: string;
  /** A draft: nothing to reply to or forward. */
  draft?: boolean;
  /** In a message window of its own: no AI (the panel is in the main window), no "open in window". */
  inWindow?: boolean;
  /**
   * The message the AI panel works on for this pane, if not `messageId`: for a conversation row
   * the row's message, while the pane opens another one of the thread (the oldest unread).
   */
  aiMessageId?: MessageId;
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
  title,
  draft = false,
  inWindow = false,
  aiMessageId,
}: ReadingPaneProps) {
  const { t } = useTranslation();
  // The chosen format belongs to its message: another message of the conversation starts on "auto".
  const [chosenFormat, setChosenFormat] = useState<{ messageId: MessageId; format: BodyFormat } | null>(null);
  const format: BodyFormat = chosenFormat?.messageId === messageId ? chosenFormat.format : "auto";
  const setFormat = (next: BodyFormat) => {
    setChosenFormat({ messageId, format: next });
  };
  // Stage 21 (decision 0066): in the dark theme the core adapts the mail's colours; "Original
  // colours" belongs to its message, like the chosen format.
  const theme = useAppliedTheme();
  const [originalFor, setOriginalFor] = useState<MessageId | null>(null);
  const showOriginal = originalFor === messageId;
  const scheme = mailScheme(theme, showOriginal);
  const query = useMessageView(account.id, messageId, format, scheme);
  const linkRequest = useUiStore((state) => state.linkRequest);
  const articleRef = useRef<HTMLElement>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const subjectId = useId();
  const handledFocus = useRef<number | null>(null);
  const view = query.data?.id === messageId ? query.data : undefined;
  const skeleton = useDelayed(query.isPending, SKELETON_DELAY_MS);
  // Stage 19: "all headers" belongs to its message, like the chosen format.
  const [headersFor, setHeadersFor] = useState<MessageId | null>(null);
  const allHeaders = headersFor === messageId;

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
  const canShowOriginal = theme === "dark" && body?.kind === "html" && (showOriginal || body.colors !== "original");
  // Paper is white: an adapted mail prints in its original colours (and stays so on screen).
  const beforePrint = async () => {
    if (theme !== "dark" || body?.kind !== "html" || body.colors === "original") return;
    setOriginalFor(messageId);
    await whenFrameShows(articleRef.current, "original");
  };
  const link = linkRequest && renderToken !== null && linkRequest.renderToken === renderToken ? linkRequest : null;
  const sender = view?.header.from.find((address) => address.email.trim() || address.name?.trim());

  const retry = () => {
    void refetch();
  };
  const backToMessage = () => {
    articleRef.current?.focus({ preventScroll: true });
  };
  // Stage 16: Cmd/Ctrl+F in the pane finds in the message (elsewhere it stays the mail search).
  const isFindShortcut = useFindShortcut();
  const [finding, setFinding] = useState(0);
  const findReturn = useRef<HTMLElement | null>(null);
  const closeFind = () => {
    setFinding(0);
    const back = findReturn.current;
    findReturn.current = null;
    if (back?.isConnected) back.focus({ preventScroll: true });
    else backToMessage();
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (!event.defaultPrevented && isFindShortcut(event.nativeEvent)) {
      event.preventDefault();
      if (finding === 0) findReturn.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      setFinding((seq) => seq + 1);
      return;
    }
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    onEscape();
  };

  let card;
  if (query.isError) {
    card = <ErrorNotice context={t("reading.loadFailed")} error={query.error} onRetry={retry} data-testid="reading-error" />;
  } else if (!view) {
    card = (
      <div className={CARD}>
        <div className="px-5 py-5">
          <Skeleton label={t("reading.loading")} visible={skeleton} />
        </div>
      </div>
    );
  } else {
    card = (
      <div className={CARD} data-testid="message-card">
        <div className="flex flex-col gap-4 px-4 pt-4 pb-2 min-[72rem]:px-5">
          <div className="flex flex-col gap-3">
            <MessageHeader
              header={view.header}
              account={account}
              subjectId={subjectId}
              showSubject={false}
              menu={
                <CardMenu
                  account={account}
                  messageId={view.id}
                  inWindow={inWindow}
                  draft={draft}
                  allHeaders={allHeaders}
                  onToggleAllHeaders={() => {
                    setHeadersFor(allHeaders ? null : view.id);
                  }}
                />
              }
              collapsible
            />
            {allHeaders && (
              <AllHeaders
                messageId={view.id}
                onHide={() => {
                  setHeadersFor(null);
                }}
              />
            )}
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
          {/* Stage 16: the sender is blocked (future mail goes to Junk or is hidden). */}
          <BlockedNote from={view.header.from} />
          {/* Stage 16: a request for a read receipt (answered only by a click), receipts that came back. */}
          <ReadReceipts account={account} messageId={view.id} bodyShown={shownFormat !== null} />
          <Problems view={view} />
          <Attachments messageId={view.id} attachments={view.attachments} partial={view.partial} />
          {/* HTML or plain text, and (stage 21) the original colours in the dark theme: small
              switches at the body they change, not in the toolbar. */}
          {((canToggle && shownFormat) || canShowOriginal) && (
            <div className="-mb-2 flex flex-wrap justify-end gap-2" data-print-hide>
              {canShowOriginal && (
                <ColorsToggle
                  original={showOriginal}
                  onChange={(original) => {
                    setOriginalFor(original ? messageId : null);
                  }}
                />
              )}
              {canToggle && shownFormat && <FormatToggle shown={shownFormat} onChange={setFormat} />}
            </div>
          )}
          <Body view={view} account={account} format={format} scheme={scheme} scrollerRef={scrollerRef} onRetry={retry} />
        </div>
        <div className="flex flex-wrap items-center justify-between gap-2 px-3 pb-3" data-print-hide>
          {draft ? <span /> : <ReplyRow messageId={view.id} sender={(sender && addressName(sender)) ?? t("list.unknownSender")} />}
          {!inWindow && <QuickReplies account={account} messageId={aiMessageId ?? view.id} />}
        </div>
        {/* Stage 16: the inline quick reply (the same drafts and outbox as the composer). */}
        {!draft && !inWindow && (
          <div className="px-3 pb-3">
            <QuickReply
              key={view.id}
              account={account}
              messageId={view.id}
              sender={(sender && addressName(sender)) ?? t("list.unknownSender")}
              onLeave={backToMessage}
            />
          </div>
        )}
      </div>
    );
  }
  const subject = view?.header.subject ?? title;

  return (
    <section
      aria-label={t("reading.region")}
      onKeyDown={onKeyDown}
      data-testid="reading-pane"
      data-find-scope
      className="relative flex min-h-0 min-w-0 flex-1 flex-col bg-surface"
    >
      <div className="flex h-12 shrink-0 items-center gap-1 border-b border-border-subtle ps-3 pe-2">
        <div className="flex min-w-0 flex-1 items-center">{toolbar}</div>
        {query.isFetching && view && (
          <LoaderCircle aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
        )}
        {/* Stage 9: save as .eml, print (the print styles show only the article below). */}
        {view && <MessageTools key={view.id} messageId={view.id} beforePrint={beforePrint} shown={!query.isPending && body?.kind !== "loading"} />}
        <span aria-hidden className="mx-1 h-5 w-px shrink-0 bg-border" />
        <IconButton icon={X} label={t("reading.close")} onClick={onClose} />
      </div>
      {finding > 0 && <FindBar scopeRef={scrollerRef} focusSeq={finding} onClose={closeFind} />}
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
          {card}
        </article>
      </div>
      {link && <LinkCard key={link.seq} request={link} onClosed={backToMessage} onReload={retry} />}
      {!inWindow && <AskBar account={account} />}
    </section>
  );
}

/** A message as a card on the reading surface: raised, rounded, a soft shadow instead of a frame. */
const CARD = "flex flex-col rounded-card border border-border-subtle bg-surface-raised shadow-card";
