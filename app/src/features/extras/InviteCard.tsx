import { CalendarDays, ExternalLink } from "lucide-react";
import { useId, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Notice } from "../../components/Notice";
import type { CalendarEvent, CalendarInviteInfo, CalendarPerson, CalendarProblem, MessageId } from "../../services/ipc";
import { describeRecurrence, eventWhen, formatDay, inviteKind, type Translate } from "./calendar";
import { useOpenInvite } from "./queries";

/** Attendees named on the card before "and n more". */
const NAMED_ATTENDEES = 3;

function personText(person: CalendarPerson, t: Translate): string {
  const name = person.name?.trim();
  const email = person.email?.trim();
  if (name && email && name !== email) return t("extras.invite.person", { name, email });
  return name || email || t("extras.invite.unknownPerson");
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,3fr)] gap-3 text-13">
      <dt className="text-secondary">{label}</dt>
      <dd className="min-w-0 break-words text-primary select-text">{children}</dd>
    </div>
  );
}

function EventDetails({ event, method, language, t }: { event: CalendarEvent; method: string | null; language: string; t: Translate }) {
  const when = eventWhen(event, language, t);
  const occurrence = event.recurrenceId ? formatDay(event.recurrenceId, language) : null;
  const attendees = event.attendees.map((person) => personText(person, t));
  const named = attendees.slice(0, NAMED_ATTENDEES);
  const more = Math.max(0, event.attendeeCount - named.length);
  const namedText = new Intl.ListFormat(language, { type: "conjunction" }).format(named);
  // An answer (iTIP REPLY) carries the one attendee who answered and how.
  const answer = method === "REPLY" ? event.attendees[0] : undefined;
  return (
    <div className="flex flex-col gap-3" data-testid="invite-event">
      {occurrence && <p className="text-13 text-secondary">{t("extras.invite.occurrence", { date: occurrence })}</p>}
      {answer?.status && (
        <p className="text-13 font-medium text-primary" data-testid="invite-answer">
          {t(`extras.invite.reply.${answer.status}`, { name: personText(answer, t) })}
        </p>
      )}
      <dl className="flex flex-col gap-2">
        <Row label={t("extras.invite.when")}>
          {when ? (
            <span className="flex flex-col gap-1">
              <span data-testid="invite-when">{when.text}</span>
              {when.original && (
                <span className="text-secondary" data-testid="invite-original">
                  {t("extras.invite.original", when.original)}
                </span>
              )}
              {when.floating && (
                <span className="text-secondary" data-testid="invite-floating">
                  {t("extras.invite.floating")}
                </span>
              )}
            </span>
          ) : (
            <span className="text-secondary" data-testid="invite-no-time">
              {t("extras.invite.noTime")}
            </span>
          )}
        </Row>
        {event.recurrence && (
          <Row label={t("extras.invite.repeats")}>
            <span data-testid="invite-recurrence">{describeRecurrence(event.recurrence, language, t)}</span>
          </Row>
        )}
        {event.location && (
          <Row label={t("extras.invite.where")}>
            <span data-testid="invite-location">{event.location}</span>
          </Row>
        )}
        {event.organizer && (
          <Row label={t("extras.invite.organizer")}>
            <span data-testid="invite-organizer">{personText(event.organizer, t)}</span>
          </Row>
        )}
        {event.attendeeCount > 0 && method !== "REPLY" && (
          <Row label={t("extras.invite.attendees")}>
            <span data-testid="invite-attendees">
              {more > 0 ? t("extras.invite.attendeesMore", { names: namedText, count: more }) : namedText}
            </span>
          </Row>
        )}
      </dl>
      {event.description && (
        <details className="text-13">
          <summary className="w-fit cursor-pointer rounded-control font-medium text-accent">{t("extras.invite.description")}</summary>
          <p className="mt-2 break-words whitespace-pre-wrap text-primary select-text" data-testid="invite-description">
            {event.description}
          </p>
        </details>
      )}
    </div>
  );
}

function Problems({ problems, t }: { problems: readonly CalendarProblem[]; t: Translate }) {
  if (problems.length === 0) return null;
  // One line per kind of problem; a damaged file can repeat the same one many times.
  const unique = problems.filter((problem, index) => problems.findIndex((p) => p.code === problem.code) === index);
  return (
    <Notice tone="warning" title={t("extras.invite.problemsTitle")} data-testid="invite-problems">
      <ul className="flex list-disc flex-col gap-1 ps-4">
        {unique.map((problem) => (
          <li key={problem.code} data-problem={problem.code}>
            {t(`extras.invite.problems.${problem.code}`, { detail: problem.detail ?? "" })}
          </li>
        ))}
      </ul>
    </Notice>
  );
}

interface InviteCardProps {
  messageId: MessageId;
  info: CalendarInviteInfo;
}

/**
 * A calendar part of the message as a card (stage 9): what, when in the user's own zone (and
 * the organizer's time when it differs), repetition in words, where, who; damaged parts named.
 * "Open in calendar" hands the file to the system's calendar app. Answering is the calendar
 * app's job: the core sends no iTIP replies (docs/stages/09, "Nicht in dieser Stufe").
 */
export function InviteCard({ messageId, info }: InviteCardProps) {
  const { t, i18n } = useTranslation();
  const text = t as unknown as Translate;
  const open = useOpenInvite(messageId);
  const titleId = useId();
  const { invite } = info;
  const [first, ...others] = invite.events;
  const kind = inviteKind(invite, first);
  const title = first?.summary?.trim() || t("extras.invite.untitled");
  const tentative = first?.status === "tentative";
  const answerable = invite.method === "REQUEST" && kind !== "cancel";

  return (
    <section
      aria-labelledby={titleId}
      className="flex flex-col gap-4 rounded-card border border-border bg-surface-raised p-4"
      data-testid="invite-card"
      data-kind={kind}
    >
      <header className="flex items-start gap-3">
        <span className="flex size-8 shrink-0 items-center justify-center rounded-control bg-accent-soft text-accent">
          <CalendarDays aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
        </span>
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <p className={kind === "cancel" ? "text-12 font-semibold text-danger" : "text-12 font-semibold text-secondary"} data-testid="invite-kind">
            {tentative ? t("extras.invite.tentative", { kind: t(`extras.invite.kind.${kind}`) }) : t(`extras.invite.kind.${kind}`)}
          </p>
          <h3
            id={titleId}
            className={kind === "cancel" ? "text-16 font-semibold break-words text-primary line-through" : "text-16 font-semibold break-words text-primary"}
            data-testid="invite-title"
          >
            {title}
          </h3>
        </div>
      </header>
      {first ? (
        <EventDetails event={first} method={invite.method} language={i18n.language} t={text} />
      ) : (
        <p className="text-13 text-secondary">{t("extras.invite.noEvents")}</p>
      )}
      {others.map((event, index) => (
        <div key={index} className="flex flex-col gap-2 border-t border-border pt-3">
          {event.summary?.trim() && event.summary.trim() !== title && (
            <p className="text-13 font-semibold text-primary">{event.summary.trim()}</p>
          )}
          <EventDetails event={event} method={invite.method} language={i18n.language} t={text} />
        </div>
      ))}
      <Problems problems={invite.problems} t={text} />
      <div className="flex flex-wrap items-center gap-3" data-print-hide>
        <Button
          variant="secondary"
          disabled={open.isPending}
          onClick={() => {
            open.mutate(info.partId);
          }}
          data-testid="invite-open"
        >
          <ExternalLink aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t("extras.invite.open")}
        </Button>
        {open.isSuccess && (
          <p role="status" className="text-13 text-secondary" data-testid="invite-opened">
            {t("extras.invite.opened")}
          </p>
        )}
      </div>
      {answerable && <p className="text-12 text-tertiary">{t("extras.invite.answerHint")}</p>}
      {open.isError && <ErrorNotice context={t("extras.invite.openFailed")} error={open.error} data-testid="invite-open-error" />}
    </section>
  );
}
