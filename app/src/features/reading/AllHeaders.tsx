import { useQuery } from "@tanstack/react-query";
import { LoaderCircle, X } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import { sourceIpc, type MessageId } from "../../services/ipc";

/** The header fields of a message do not change: loaded once per message while shown. */
export function useMessageHeaders(messageId: MessageId) {
  return useQuery({
    queryKey: ["messageHeaders", messageId],
    queryFn: () => sourceIpc.getHeaders(messageId),
    staleTime: Infinity,
  });
}

interface AllHeadersProps {
  messageId: MessageId;
  onHide: () => void;
}

/**
 * Stage 19 (decision 0055): "All headers" under the head of the message card: every field of the
 * message's header in order, as text (the core unfolds and decodes them and shows hidden
 * characters as visible symbols). Long lists scroll inside the card.
 */
export function AllHeaders({ messageId, onHide }: AllHeadersProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const query = useMessageHeaders(messageId);
  return (
    <section aria-labelledby={titleId} className="flex flex-col gap-2 rounded-card border border-border-subtle bg-canvas p-3" data-testid="all-headers">
      <div className="flex items-center justify-between gap-2">
        <h3 id={titleId} className="text-13 font-semibold text-primary">
          {t("source.allHeaders.title")}
        </h3>
        <IconButton icon={X} label={t("source.allHeaders.hide")} onClick={onHide} />
      </div>
      {query.isError ? (
        <ErrorNotice
          context={t("source.allHeaders.failed")}
          error={query.error}
          onRetry={() => {
            void query.refetch();
          }}
        />
      ) : !query.data ? (
        <p role="status" className="flex items-center gap-2 text-12 text-secondary">
          <LoaderCircle aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t("source.allHeaders.loading")}
        </p>
      ) : query.data.length === 0 ? (
        <p className="text-12 text-secondary">{t("source.allHeaders.empty")}</p>
      ) : (
        <dl className="grid max-h-(--ui-picker-list-height) grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 overflow-y-auto font-mono text-12" data-testid="all-headers-list">
          {query.data.map((field, index) => (
            <div key={`${String(index)}-${field.name}`} className="contents">
              <dt className="text-tertiary select-text">{field.name}</dt>
              <dd className="min-w-0 break-words text-primary select-text">{field.value}</dd>
            </div>
          ))}
        </dl>
      )}
    </section>
  );
}
