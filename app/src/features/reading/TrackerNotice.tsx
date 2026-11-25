import { ChevronDown, ShieldCheck } from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import type { TrackerSummary } from "../../services/ipc";

/** Total of the trackers the core counted in one rendering. */
export function trackerTotal(trackers: TrackerSummary): number {
  return trackers.pixels + trackers.knownHosts + trackers.parameters;
}

/**
 * Stage 20 (decision 0058): how many trackers the core blocked in this message, with what that
 * means on request. Pixels and images of known tracking services never load, not even with
 * "Load once"; an image whose address carried tracking parameters loads without them. Shown only
 * when there were any.
 */
export function TrackerNotice({ trackers }: { trackers: TrackerSummary }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const detailsId = useId();
  const total = trackerTotal(trackers);
  if (total === 0) return null;
  const parts: string[] = [];
  if (trackers.pixels > 0) parts.push(t("reading.trackers.pixels", { count: trackers.pixels }));
  if (trackers.knownHosts > 0) parts.push(t("reading.trackers.knownHosts", { count: trackers.knownHosts }));
  if (trackers.parameters > 0) parts.push(t("reading.trackers.parameters", { count: trackers.parameters }));
  return (
    <div data-print-hide data-testid="tracker-notice" data-total={total} className="flex flex-col gap-2 text-13">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={detailsId}
        onClick={() => {
          setOpen((value) => !value);
        }}
        data-testid="tracker-notice-toggle"
        className="flex w-fit items-center gap-2 rounded-control px-1 py-0.5 text-secondary hover:bg-hover hover:text-primary"
      >
        <ShieldCheck aria-hidden className="size-(--ui-icon-small) shrink-0 text-success" strokeWidth={1.75} />
        <span>{t("reading.trackers.blocked", { count: total })}</span>
        <ChevronDown aria-hidden className={cx("size-(--ui-icon-small) shrink-0", open && "rotate-180")} strokeWidth={1.75} />
      </button>
      <div id={detailsId} hidden={!open} className="flex flex-col gap-2 rounded-card border border-border bg-canvas p-3" data-testid="tracker-notice-details">
        <p className="text-primary">{t("reading.trackers.explanation")}</p>
        <ul className="flex list-disc flex-col gap-1 ps-5 text-secondary">
          {parts.map((part) => (
            <li key={part}>{part}</li>
          ))}
        </ul>
        {trackers.hosts.length > 0 && (
          <p className="break-words text-secondary" data-testid="tracker-hosts">
            {t("reading.trackers.hosts", { hosts: trackers.hosts.join(", ") })}
          </p>
        )}
        <p className="text-secondary">{t("reading.trackers.setting")}</p>
      </div>
    </div>
  );
}
