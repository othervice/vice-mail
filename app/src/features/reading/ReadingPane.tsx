import { AppWindow, CloudOff, FileX, LoaderCircle, Paperclip, ReplyAll, X } from "lucide-react";
// Stage 19 (decision 0055): source, all headers, redirect, edit as new.
import { CopyPlus, CornerUpRight, FileCode, ListTree } from "lucide-react";
import { useNoticeStore } from "../../stores/notices";
import { useRedirectStore } from "../../stores/redirect";
import { openSourceInWindow } from "../windows/open";
import { AllHeaders } from "./AllHeaders";
import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import { Notice } from "../../components/Notice";
import { useToolbarSlot } from "../../components/ToolbarPortal";
import type { Account, BodyFormat, MailboxId, MailScheme, MessageId, MessageListItem, MessageView } from "../../services/ipc";
import { openComposer } from "../../stores/composer";
import { useContactPanelStore } from "../../stores/contactPanel";
import { useUiStore } from "../../stores/ui";
import { accountFill } from "../accounts/accountColor";
import { OverflowMenu, type OverflowItem } from "../actions/OverflowMenu";
import { AskBar, QuickReplies } from "../ai/ReadingAi";
import { useMailboxes } from "../mailboxes/queries";
import { dayBounds, formatFullDate, formatListDate } from "../messages/rows";
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
// Stage 22 (decision 0077): the learning spam filter's verdict.
import { SpamNote } from "../spam/SpamNote";
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
      // Stage 23 (decision 0084): quiet, at the body it changes.
      className="inline-flex shrink-0 gap-1"
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
          className="h-6 rounded-control px-2 text-12 font-medium text-tertiary transition-colors hover:text-primary aria-pressed:bg-badge aria-pressed:text-primary"
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
  return <OverflowMenu label={t("reading.card.menu")} items={items} look="toolbar" data-testid="card-menu" />;
}

/**
 * Stage 23, step 3 (decision 0084): the conversation around the open message, as in the draft:
 * every other message of it as one line (sender, the start of its text, its day); the earlier
 * ones above the message, the later ones below it. A click (Enter, Space) opens that message in
 * the pane instead; the list's conversation rows stay as they are (decision 0049).
 */
export interface ConversationRows {
  messages: readonly MessageListItem[];
  /** Opens another message of the conversation in the pane. */
  onOpen: (message: MessageListItem) => void;
}

function ConversationLine({ message, own, onOpen }: { message: MessageListItem; own: string; onOpen: () => void }) {
  const { t, i18n } = useTranslation();
  const bounds = dayBounds(new Date(), 1);
  const from = message.from;
  const name = !from ? t("list.unknownSender") : from.email.trim().toLowerCase() === own ? t("reading.conversation.you") : (addressName(from) ?? from.email);
  const text = message.preview?.trim() || message.subject.trim() || t("list.noSubject");
  return (
    <button
      type="button"
      onClick={onOpen}
      aria-label={t("reading.conversation.open", { name, date: formatFullDate(message.date, i18n.language) })}
      className="flex w-full min-w-0 items-center gap-3 border-t border-border-subtle py-3 text-start transition-colors hover:bg-hover"
      data-testid="conversation-line"
    >
      <span className={cx("w-(--ui-conversation-name-width) shrink-0 truncate text-13 text-primary", message.unread ? "font-bold" : "font-semibold")}>
        {name}
      </span>
      <span className="min-w-0 flex-1 truncate text-13 text-tertiary">{text}</span>
      <span className="shrink-0 text-12 text-tertiary tabular-nums">{formatListDate(message.date, bounds, i18n.language)}</span>
    </button>
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
  /** Stage 23: the folder the message lies in, named above the subject with the account. */
  mailboxId?: MailboxId;
  /** Stage 23: the other messages of the conversation, as lines around the message. */
  conversation?: ConversationRows;
}

/**
 * Stage 23, step 3 (decision 0084): who the contact panel shows for the open message: the sender,
 * or for the user's own mail the first other recipient.
 */
function panelPerson(view: MessageView, account: Account): { email: string; name: string | null } | null {
  const own = account.email.trim().toLowerCase();
  const usable = (address: { email: string }) => address.email.trim() !== "" && address.email.trim().toLowerCase() !== own;
  const from = view.header.from.find((address) => address.email.trim() !== "");
  const person = from && usable(from) ? from : [...view.header.to, ...view.header.cc].find(usable);
  return person ? { email: person.email.trim(), name: person.name?.trim() || null } : null;
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
  mailboxId,
  conversation,
}: ReadingPaneProps) {
  const { t } = useTranslation();
  // Stage 23 (decision 0084): the actions stand in the toolbar across the window.
  const toolbarSlot = useToolbarSlot("reading");
  const inToolbar = toolbarSlot !== null && !inWindow;
  const mailbox = useMailboxes(account.id).data?.find((candidate) => candidate.id === mailboxId);
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
  // Stage 23: a message opened from the conversation's lines takes the focus (its line is gone).
  const focusOpened = useRef(false);
  useEffect(() => {
    if (!focusOpened.current) return;
    focusOpened.current = false;
    articleRef.current?.focus({ preventScroll: true });
  }, [messageId]);

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

  // Stage 23: the contact panel shows the other side of the message in the main window's pane.
  const showPerson = useContactPanelStore((state) => state.show);
  const leavePerson = useContactPanelStore((state) => state.leave);
  const person = view && !inWindow ? panelPerson(view, account) : null;
  const personEmail = person?.email ?? null;
  const personName = person?.name ?? null;
  useEffect(() => {
    if (personEmail === null) return undefined;
    showPerson({ email: personEmail, name: personName, accountId: account.id });
    return () => {
      leavePerson(personEmail);
    };
  }, [personEmail, personName, account.id, showPerson, leavePerson]);

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
      <div className={MESSAGE}>
        <Skeleton label={t("reading.loading")} visible={skeleton} />
      </div>
    );
  } else {
    card = (
      // Stage 23 (decision 0084): no card any more; the message lies on the pane's surface
      // below a hairline, as in the draft. The test id stays the message's.
      <div className={MESSAGE} data-testid="message-card">
        <div className="flex flex-col gap-4">
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
          <SpamNote account={account} messageId={view.id} />
          <BlockedNote from={view.header.from} />
          {/* Stage 16: a request for a read receipt (answered only by a click), receipts that came back. */}
          <ReadReceipts account={account} messageId={view.id} bodyShown={shownFormat !== null} />
          <Problems view={view} />
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
          {/* Stage 23 (decision 0084): the attachments below the text, as cards, as in the draft. */}
          <Attachments messageId={view.id} attachments={view.attachments} partial={view.partial} />
        </div>
        {/* Stage 13: the AI's reply chips (replying itself is in the toolbar and below). */}
        {!inWindow && (
          <div className="flex flex-wrap items-center gap-2 empty:hidden" data-print-hide>
            <QuickReplies account={account} messageId={aiMessageId ?? view.id} />
          </div>
        )}
      </div>
    );
  }
  const subject = view?.header.subject ?? title;
  const own = account.email.trim().toLowerCase();
  const openIndex = conversation ? conversation.messages.findIndex((message) => message.id === messageId) : -1;
  const openLine = (message: MessageListItem) => {
    focusOpened.current = true;
    conversation?.onOpen(message);
  };
  const lines = (part: "before" | "after") =>
    conversation?.messages
      .filter((_, index) => (part === "before" ? index < openIndex : openIndex !== -1 && index > openIndex))
      .map((message) => (
        <ConversationLine
          key={message.id}
          message={message}
          own={own}
          onOpen={() => {
            openLine(message);
          }}
        />
      ));
  const accountName = account.displayName || account.email;
  const folderName = mailbox ? (mailbox.role === "custom" ? mailbox.name : t(`roles.${mailbox.role}`)) : null;
  const count = conversation && conversation.messages.length > 1 ? conversation.messages.length : null;
  const fetching = query.isFetching && view !== undefined;
  // Stage 9: save as .eml, print (the print styles show only the article below).
  const tools = view && (
    <MessageTools key={view.id} messageId={view.id} beforePrint={beforePrint} shown={!query.isPending && body?.kind !== "loading"} small={inToolbar} />
  );
  const closeButton = <IconButton icon={X} label={t("reading.close")} onClick={onClose} small={inToolbar} />;

  return (
    <section
      aria-label={t("reading.region")}
      onKeyDown={onKeyDown}
      data-testid="reading-pane"
      data-find-scope
      className="@container/pane relative flex min-h-0 min-w-0 flex-1 flex-col bg-surface"
    >
      {inToolbar ? (
        // Stage 23 (decision 0084): the open message's actions in the toolbar across the window.
        toolbar &&
        createPortal(
          <div className="flex min-w-0 flex-1 items-center" data-testid="reading-toolbar">
            {toolbar}
          </div>,
          toolbarSlot,
        )
      ) : (
        // A message window (or no toolbar): the pane's own bar with the actions, as before.
        <div className="flex shrink-0 items-center gap-1 border-b border-border-subtle py-1 ps-3 pe-2">
          <div className="flex min-w-0 flex-1 items-center">{toolbar}</div>
          {fetching && <LoaderCircle aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />}
          {tools}
          <span aria-hidden className="mx-1 h-5 w-px shrink-0 bg-border" />
          {closeButton}
        </div>
      )}
      {finding > 0 && <FindBar scopeRef={scrollerRef} focusSeq={finding} onClose={closeFind} />}
      <div ref={scrollerRef} className="min-h-0 flex-1 overflow-y-auto" data-testid="reading-scroller">
        <article
          ref={articleRef}
          tabIndex={-1}
          aria-labelledby={subject !== undefined ? subjectId : undefined}
          aria-label={subject !== undefined ? undefined : t("reading.region")}
          aria-busy={query.isPending || body?.kind === "loading"}
          data-print-root
          // Stage 23 (decision 0084): no card; 720 px of text from the pane's start, 40 px in,
          // as in the draft (24 px in a narrow pane).
          className="flex w-full max-w-(--ui-reading-column) flex-col px-6 pt-6 pb-8 focus-visible:-outline-offset-2 @min-[40rem]/pane:px-10 @min-[40rem]/pane:pt-8"
        >
          {/* Account · folder · messages of the conversation; the pane's quiet tools at the end. */}
          <div className="flex min-h-8 items-center gap-2 text-12 text-tertiary" data-testid="reading-meta" data-print-hide>
            <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-mark", accountFill(account.color))} />
            <span className="min-w-0 truncate" data-testid="reading-meta-place">
              {folderName ? t("reading.meta.place", { account: accountName, folder: folderName }) : accountName}
            </span>
            {count !== null && (
              <>
                <span aria-hidden>{t("reading.header.dot")}</span>
                <span className="shrink-0 tabular-nums" data-testid="reading-meta-count">
                  {t("reading.meta.count", { count })}
                </span>
              </>
            )}
            <span className="flex-1" />
            {inToolbar && (
              <div className="flex shrink-0 items-center gap-1">
                {fetching && <LoaderCircle aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />}
                {tools}
                {closeButton}
              </div>
            )}
          </div>
          {subject !== undefined && <SubjectTitle subject={subject} subjectId={subjectId} />}
          <div className="mt-5 flex flex-col">
            {lines("before")}
            {card}
            {lines("after")}
          </div>
        </article>
      </div>
      {/* Stage 16: the inline quick reply (the same drafts and outbox as the composer); stage 23:
          a framed field at the bottom of the pane, as in the draft. */}
      {view && !draft && !inWindow && (
        <div className="w-full max-w-(--ui-reading-column) shrink-0 px-6 pt-3 pb-5 @min-[40rem]/pane:px-10" data-print-hide>
          <QuickReply
            key={view.id}
            account={account}
            messageId={view.id}
            sender={(sender && addressName(sender)) ?? t("list.unknownSender")}
            onLeave={backToMessage}
          />
        </div>
      )}
      {link && <LinkCard key={link.seq} request={link} onClosed={backToMessage} onReload={retry} />}
      {!inWindow && <AskBar account={account} raised={view !== undefined && !draft} />}
    </section>
  );
}

/** Stage 23 (decision 0084): the open message below a hairline on the pane's surface, no card. */
const MESSAGE = "flex flex-col gap-4 border-t border-border-subtle pt-4";
