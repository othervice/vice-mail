import { X } from "lucide-react";
import { useId, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import type { CoreError, MessageId, NewMailCard, NewMailPopup } from "../../services/ipc";
import { accountFill } from "../accounts/accountColor";
import { dayBounds, firstDayOfWeek, formatFullDate, formatListDate } from "../messages/rows";

/** A card whose button is waiting for the core, or whose last action failed. */
export interface CardState {
  busy?: boolean;
  error?: CoreError | null;
}

export interface NewMailPopupViewProps {
  popup: NewMailPopup;
  /** Per message: a running action or the error of the last one. */
  states?: ReadonlyMap<MessageId, CardState>;
  /** What the live region says (the newest arrivals), empty when nothing is new. */
  announcement: string;
  /** The current time, for the time of each card (today: the time, else the date). */
  now: Date;
  onRead: (card: NewMailCard) => void;
  onDelete: (card: NewMailCard) => void;
  onUndo: (card: NewMailCard) => void;
  onClose: (card: NewMailCard) => void;
  onOpenInbox: () => void;
}

/**
 * Stage 18 (decision 0051): the popup for new mail, drawn in its own window at the bottom right
 * of the screen. One card per message, newest on top, at most three, then a line "N more" that
 * opens the inbox. A card shows the sender's name (no picture, stage 22, decision 0072), subject,
 * two lines of the snippet as plain text, the time and, where there are several accounts, the
 * account; its
 * buttons are "Read", "Delete" (disabled with the reason for a read-only account) and close.
 * With hidden content a card only says "New message" and offers "Read". A deleted card says
 * "Deleted" with "Undo". The window never takes the focus; a polite live region tells screen
 * readers what arrived.
 */
export function NewMailPopupView({
  popup,
  states,
  announcement,
  now,
  onRead,
  onDelete,
  onUndo,
  onClose,
  onOpenInbox,
}: NewMailPopupViewProps) {
  const { t, i18n } = useTranslation();
  const language = i18n.language;
  const bounds = useMemo(() => dayBounds(now, firstDayOfWeek(language)), [now, language]);
  const headingId = useId();
  return (
    <main aria-labelledby={headingId} data-testid="new-mail-popup" className="flex flex-col bg-surface-raised text-primary">
      <h1 id={headingId} className="sr-only">
        {t("popup.title")}
      </h1>
      <div role="status" aria-live="polite" className="sr-only" data-testid="popup-announce">
        {announcement}
      </div>
      <ul className="flex flex-col divide-y divide-border-subtle">
        {popup.cards.map((card) => (
          <li key={card.messageId}>
            <Card
              card={card}
              state={states?.get(card.messageId)}
              time={formatListDate(card.date, bounds, language)}
              fullTime={formatFullDate(card.date, language)}
              onRead={onRead}
              onDelete={onDelete}
              onUndo={onUndo}
              onClose={onClose}
            />
          </li>
        ))}
      </ul>
      {popup.more > 0 && (
        <button
          type="button"
          onClick={onOpenInbox}
          data-testid="popup-more"
          className="border-t border-border-subtle px-4 py-3 text-left text-13 font-medium text-accent transition-colors hover:bg-hover"
        >
          {t("popup.more", { count: popup.more })}
        </button>
      )}
    </main>
  );
}

interface CardProps {
  card: NewMailCard;
  state: CardState | undefined;
  time: string;
  fullTime: string;
  onRead: (card: NewMailCard) => void;
  onDelete: (card: NewMailCard) => void;
  onUndo: (card: NewMailCard) => void;
  onClose: (card: NewMailCard) => void;
}

/** The sender's name, else the address, else "Unknown sender"; "New message" when hidden. */
export function cardTitle(card: NewMailCard, t: (key: "popup.newMessage" | "popup.noSender") => string): string {
  const content = card.content;
  if (!content) return t("popup.newMessage");
  return content.senderName ?? content.senderEmail ?? t("popup.noSender");
}

function Card({ card, state, time, fullTime, onRead, onDelete, onUndo, onClose }: CardProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const reasonId = useId();
  const content = card.content;
  const busy = state?.busy ?? false;
  const error = state?.error ?? null;
  const title = cardTitle(card, t);

  if (card.deleted) {
    return (
      <article
        aria-labelledby={titleId}
        data-testid="popup-card"
        data-message-id={card.messageId}
        data-deleted=""
        className="flex items-center gap-3 px-4 py-3"
      >
        <p id={titleId} className="min-w-0 flex-1 truncate text-13 text-secondary" data-testid="popup-deleted">
          {t("popup.deleted")}
          {content ? <span className="text-tertiary">{` · ${content.subject || t("popup.noSubject")}`}</span> : null}
        </p>
        <Button variant="quiet" onClick={() => { onUndo(card); }} disabled={busy} data-testid="popup-undo">
          {t("popup.undo")}
        </Button>
        <CloseButton card={card} onClose={onClose} />
        {error && <ErrorLine error={error} />}
      </article>
    );
  }

  return (
    <article
      aria-labelledby={titleId}
      data-testid="popup-card"
      data-message-id={card.messageId}
      className="flex px-4 pt-3 pb-3"
    >
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex items-center gap-2">
          <h2 id={titleId} className="min-w-0 flex-1 truncate text-14 font-semibold text-primary" data-testid="popup-sender">
            {title}
          </h2>
          <time dateTime={new Date(card.date).toISOString()} title={fullTime} className="shrink-0 text-12 text-secondary tabular-nums">
            {time}
          </time>
          <CloseButton card={card} onClose={onClose} />
        </div>
        {content && (
          <p className="truncate text-13 font-medium text-primary" data-testid="popup-subject">
            {content.subject || t("popup.noSubject")}
          </p>
        )}
        {content?.snippet && (
          <p className="line-clamp-2 text-13 text-secondary" data-testid="popup-snippet">
            {content.snippet}
          </p>
        )}
        {card.account && (
          <p className="mt-1 flex min-w-0 items-center gap-2 text-12 text-secondary" data-testid="popup-account">
            <span aria-hidden className={cx("size-2 shrink-0 rounded-full", accountFill(card.account.color))} />
            <span className="truncate">{t("popup.account", { name: card.account.name })}</span>
          </p>
        )}
        <div className="mt-2 flex items-center gap-2">
          <Button variant="primary" onClick={() => { onRead(card); }} disabled={busy} data-testid="popup-read">
            {t("popup.read")}
          </Button>
          {content && (
            <Button
              variant="secondary"
              onClick={() => { onDelete(card); }}
              disabled={busy || !card.writesEnabled}
              {...(!card.writesEnabled && { "aria-describedby": reasonId })}
              data-testid="popup-delete"
            >
              {t("popup.delete")}
            </Button>
          )}
        </div>
        {content && !card.writesEnabled && (
          <p id={reasonId} className="mt-1 text-12 text-secondary" data-testid="popup-read-only">
            {t("popup.readOnly")}
          </p>
        )}
        {error && <ErrorLine error={error} />}
      </div>
    </article>
  );
}

function CloseButton({ card, onClose }: { card: NewMailCard; onClose: (card: NewMailCard) => void }) {
  const { t } = useTranslation();
  return (
    <button
      type="button"
      aria-label={t("popup.close")}
      onClick={() => {
        onClose(card);
      }}
      data-testid="popup-close"
      className="-my-1 -mr-2 flex size-6 shrink-0 items-center justify-center rounded-control text-secondary transition-colors hover:bg-hover hover:text-primary"
    >
      <X aria-hidden className="size-4" strokeWidth={1.75} />
    </button>
  );
}

function ErrorLine({ error }: { error: CoreError }) {
  const { t } = useTranslation();
  return (
    <p role="alert" className="mt-1 text-12 text-danger" data-testid="popup-error" data-error-code={error.code}>
      {t("popup.failed", { reason: t(`errors.codes.${error.code}.title`) })}
    </p>
  );
}
