import { ChevronDown, ChevronUp } from "lucide-react";
import { useId, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import type { Account, Address, MessageViewHeader } from "../../services/ipc";
import { accountFill } from "../accounts/accountColor";
import { useAccounts } from "../accounts/queries";
import { formatFullDate } from "../messages/rows";
import { addressName } from "./format";

/** Recipients of one field shown before "and n more". */
const COLLAPSED_RECIPIENTS = 3;

/** One address: the name, and the address itself next to it (never only on hover). */
function AddressText({ address }: { address: Address }) {
  const { t } = useTranslation();
  const name = address.name?.trim();
  const email = address.email.trim();
  if (!name && !email) return <span className="text-tertiary">{t("list.unknownSender")}</span>;
  if (!name || name === email) return <span className="text-primary">{email}</span>;
  return (
    <span>
      <span className="text-primary">{name}</span>{" "}
      {email && <span className="text-tertiary">{email}</span>}
    </span>
  );
}

function AddressList({ addresses }: { addresses: readonly Address[] }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const listId = useId();
  const collapsible = addresses.length > COLLAPSED_RECIPIENTS + 1;
  const shown = collapsible && !expanded ? addresses.slice(0, COLLAPSED_RECIPIENTS) : addresses;
  return (
    <span id={listId}>
      {shown.map((address, i) => (
        <span key={`${address.email}-${String(i)}`}>
          <AddressText address={address} />
          {i < shown.length - 1 && <span className="text-secondary">{t("reading.header.separator")}</span>}
        </span>
      ))}
      {collapsible && (
        <>
          {" "}
          <button
            type="button"
            aria-expanded={expanded}
            aria-controls={listId}
            onClick={() => {
              setExpanded((value) => !value);
            }}
            className="rounded-control text-accent hover:underline"
          >
            {expanded
              ? t("reading.header.fewer")
              : t("reading.header.more", { count: addresses.length - COLLAPSED_RECIPIENTS })}
          </button>
        </>
      )}
    </span>
  );
}

interface FieldProps {
  label: string;
  addresses: readonly Address[];
}

function AddressField({ label, addresses }: FieldProps) {
  if (addresses.length === 0) return null;
  return (
    <>
      <dt className="text-tertiary">{label}</dt>
      <dd className="min-w-0 break-words select-text">
        <AddressList addresses={addresses} />
      </dd>
    </>
  );
}

function sameAddresses(a: readonly Address[], b: readonly Address[]): boolean {
  const key = (list: readonly Address[]) =>
    list
      .map((x) => x.email.trim().toLowerCase())
      .sort()
      .join(",");
  return key(a) === key(b);
}

interface SubjectTitleProps {
  subject: string;
  subjectId: string;
}

/** The subject as the calm title above a message or a conversation; it names the article. */
export function SubjectTitle({ subject, subjectId }: SubjectTitleProps) {
  const { t } = useTranslation();
  const text = subject.trim();
  return (
    <h2
      id={subjectId}
      className={cx("text-26 font-bold tracking-title break-words select-text", text ? "text-primary" : "text-tertiary")}
      data-testid="reading-subject"
    >
      {text || t("list.noSubject")}
    </h2>
  );
}

interface RecipientSummaryProps {
  header: MessageViewHeader;
  account: Account;
  /** The account's colour and name join the line when there are several accounts. */
  withAccount: boolean;
  showReplyTo: boolean;
  open: boolean;
  detailsId: string;
  onToggle: () => void;
}

/** Recipients shown by name in the folded head before "+n". */
const SUMMARY_NAMES = 2;

/**
 * The folded head's second line: "to you, Ethan Bennett +1", a differing reply address (a common
 * sign of a forged mail stays visible), and with several accounts the account's colour and name.
 * A button: Enter or a click unfolds the full list of recipients and the account.
 */
function RecipientSummary({ header, account, withAccount, showReplyTo, open, detailsId, onToggle }: RecipientSummaryProps) {
  const { t } = useTranslation();
  const seen = new Set<string>();
  const recipients = [...header.to, ...header.cc, ...header.bcc].filter((address) => {
    const key = address.email.trim().toLowerCase() || (address.name?.trim() ?? "");
    if (!key || seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  const own = account.email.trim().toLowerCase();
  const names = recipients
    .slice(0, SUMMARY_NAMES)
    .map((address) => (address.email.trim().toLowerCase() === own ? t("reading.header.you") : (addressName(address) ?? "")))
    .join(t("reading.header.separator"));
  const rest = recipients.length - SUMMARY_NAMES;
  const text =
    recipients.length === 0
      ? t("reading.header.noRecipients")
      : rest > 0
        ? t("reading.header.summaryMore", { names, count: rest })
        : t("reading.header.summary", { names });
  const replyTo = header.replyTo[0]?.email.trim();
  const Chevron = open ? ChevronUp : ChevronDown;
  return (
    <button
      type="button"
      aria-expanded={open}
      aria-controls={detailsId}
      onClick={onToggle}
      className="flex max-w-full min-w-0 items-center gap-1 self-start rounded-control text-12 text-tertiary transition-colors hover:text-primary"
      data-testid="reading-recipients"
    >
      <span className="truncate">{text}</span>
      {showReplyTo && replyTo && (
        <span className="shrink-0 text-warning">
          {t("reading.header.dot")}
          {t("reading.header.summaryReplyTo", { address: replyTo })}
        </span>
      )}
      {withAccount && (
        <span className="flex min-w-0 shrink items-center gap-1">
          <span aria-hidden>{t("reading.header.dot")}</span>
          <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-mark", accountFill(account.color))} />
          <span className="truncate">{account.displayName || account.email}</span>
        </span>
      )}
      <Chevron aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
      <span className="sr-only">{open ? t("reading.header.detailsHide") : t("reading.header.details")}</span>
    </button>
  );
}

interface MessageHeaderProps {
  header: MessageViewHeader;
  /** The account the message belongs to; absent for an opened `.eml` file (stage 9, decision 0027). */
  account?: Account;
  /** Stage 9: the name of the opened `.eml` file, shown where the account would be. */
  fileName?: string;
  subjectId: string;
  /** False where the subject stands above the message card (reading pane, conversation). */
  showSubject?: boolean;
  /** Controls at the end of the sender line (the message card's "…" menu). */
  menu?: ReactNode;
  /**
   * The message card: recipients and account fold into one line that unfolds on request. The
   * sender's name and address always stay visible (a forged name shows its real address).
   */
  collapsible?: boolean;
}

/**
 * Head of a message (docs/DESIGN.md): subject, sender with name and address, recipients (long
 * lists folded), date and time, and the account with its colour and name. In the reading pane the
 * subject is the title above the card and this is the card's head. No avatar (stage 22, decision
 * 0072): the sender's name starts at the card's edge.
 */
export function MessageHeader({ header, account, fileName, subjectId, showSubject = true, menu, collapsible = false }: MessageHeaderProps) {
  const { t, i18n } = useTranslation();
  const [unfolded, setUnfolded] = useState(false);
  const detailsId = useId();
  const accounts = useAccounts().data ?? [];
  const folds = collapsible && account !== undefined;
  const open = !folds || unfolded;
  const sender = header.from[0];
  const senderName = sender ? addressName(sender) : null;
  const showReplyTo = header.replyTo.length > 0 && !sameAddresses(header.replyTo, header.from);
  const accountName = !account
    ? null
    : account.displayName
      ? t("nav.account", { name: account.displayName, email: account.email })
      : account.email;

  return (
    <header className="flex flex-col gap-4">
      {showSubject && <SubjectTitle subject={header.subject} subjectId={subjectId} />}
      <div className="flex items-start gap-3">
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="flex min-h-8 flex-wrap items-center justify-between gap-x-4 gap-y-1">
            <p className="min-w-0 break-words select-text" data-testid="reading-from">
              <span className="sr-only">{t("reading.header.from")} </span>
              {header.from.length === 0 || !senderName ? (
                <span className="text-tertiary">{t("list.unknownSender")}</span>
              ) : (
                header.from.map((address, i) => (
                  <span key={`${address.email}-${String(i)}`}>
                    <span className="font-semibold text-primary">{addressName(address)}</span>
                    {address.name?.trim() && address.email.trim() && (
                      <span className="text-13 text-tertiary"> {address.email}</span>
                    )}
                    {i < header.from.length - 1 && <span className="text-secondary">{t("reading.header.separator")}</span>}
                  </span>
                ))
              )}
            </p>
            {/* An opened file without a readable Date has none (0): nothing is made up. */}
            {(account || header.date > 0) && (
              <time
                dateTime={new Date(header.date).toISOString()}
                // Stage 21 (decision 0071): on a line of its own in a narrow card it wraps instead of
                // running past the card's edge.
                className="min-w-0 text-12 text-tertiary tabular-nums select-text"
                data-testid="reading-date"
              >
                {formatFullDate(header.date, i18n.language)}
              </time>
            )}
          </div>
          {folds && (
            <RecipientSummary
              header={header}
              account={account}
              withAccount={accounts.length > 1}
              showReplyTo={showReplyTo}
              open={unfolded}
              detailsId={detailsId}
              onToggle={() => {
                setUnfolded((value) => !value);
              }}
            />
          )}
          {open && (
            <dl id={detailsId} className={cx("grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-12", folds && "pt-1")} data-testid="reading-details">
              <AddressField label={t("reading.header.to")} addresses={header.to} />
              <AddressField label={t("reading.header.cc")} addresses={header.cc} />
              <AddressField label={t("reading.header.bcc")} addresses={header.bcc} />
              {showReplyTo && <AddressField label={t("reading.header.replyTo")} addresses={header.replyTo} />}
              {account && (
                <>
                  <dt className="text-tertiary">{t("reading.header.account")}</dt>
                  <dd className="flex min-w-0 items-center gap-2" data-testid="reading-account">
                    <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-mark", accountFill(account.color))} />
                    <span className="truncate text-secondary">{accountName}</span>
                  </dd>
                </>
              )}
              {fileName !== undefined && (
                <>
                  <dt className="text-tertiary">{t("eml.header.file")}</dt>
                  <dd className="min-w-0 truncate text-secondary select-text" data-testid="reading-file">
                    {fileName}
                  </dd>
                </>
              )}
            </dl>
          )}
        </div>
        {menu && <div className="-me-1 flex shrink-0">{menu}</div>}
      </div>
    </header>
  );
}
