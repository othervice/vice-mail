import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import type { Account, Address, MessageViewHeader } from "../../services/ipc";
import { accountFill } from "../accounts/accountColor";
import { Avatar } from "../accounts/Avatar";
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
      {email && <span className="text-secondary">{email}</span>}
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
      <dt className="text-secondary">{label}</dt>
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

interface MessageHeaderProps {
  header: MessageViewHeader;
  account: Account;
  subjectId: string;
}

/**
 * Head of the reading pane (docs/DESIGN.md): subject, sender with name and address, recipients
 * (long lists folded), date and time, and the account with its colour and name.
 */
export function MessageHeader({ header, account, subjectId }: MessageHeaderProps) {
  const { t, i18n } = useTranslation();
  const subject = header.subject.trim();
  const sender = header.from[0];
  const senderName = sender ? addressName(sender) : null;
  const showReplyTo = header.replyTo.length > 0 && !sameAddresses(header.replyTo, header.from);
  const accountName = account.displayName
    ? t("nav.account", { name: account.displayName, email: account.email })
    : account.email;

  return (
    <header className="flex flex-col gap-4">
      <h2
        id={subjectId}
        className={cx("text-20 font-semibold break-words select-text", subject ? "text-primary" : "text-tertiary")}
      >
        {subject || t("list.noSubject")}
      </h2>
      <div className="flex items-start gap-3">
        <Avatar name={sender?.name} email={sender?.email ?? ""} accountColor={account.color} />
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
            <p className="min-w-0 break-words select-text" data-testid="reading-from">
              <span className="sr-only">{t("reading.header.from")} </span>
              {header.from.length === 0 || !senderName ? (
                <span className="text-tertiary">{t("list.unknownSender")}</span>
              ) : (
                header.from.map((address, i) => (
                  <span key={`${address.email}-${String(i)}`}>
                    <span className="font-semibold text-primary">{addressName(address)}</span>
                    {address.name?.trim() && address.email.trim() && (
                      <span className="text-secondary"> {address.email}</span>
                    )}
                    {i < header.from.length - 1 && <span className="text-secondary">{t("reading.header.separator")}</span>}
                  </span>
                ))
              )}
            </p>
            <time
              dateTime={new Date(header.date).toISOString()}
              className="shrink-0 text-13 text-secondary tabular-nums select-text"
              data-testid="reading-date"
            >
              {formatFullDate(header.date, i18n.language)}
            </time>
          </div>
          <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-13">
            <AddressField label={t("reading.header.to")} addresses={header.to} />
            <AddressField label={t("reading.header.cc")} addresses={header.cc} />
            <AddressField label={t("reading.header.bcc")} addresses={header.bcc} />
            {showReplyTo && <AddressField label={t("reading.header.replyTo")} addresses={header.replyTo} />}
            <dt className="text-secondary">{t("reading.header.account")}</dt>
            <dd className="flex min-w-0 items-center gap-2" data-testid="reading-account">
              <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-full", accountFill(account.color))} />
              <span className="truncate text-primary">{accountName}</span>
            </dd>
          </dl>
        </div>
      </div>
    </header>
  );
}
