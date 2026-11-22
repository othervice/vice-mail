import { ImageOff } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { IpcError, RemoteContentScope } from "../../services/ipc";

interface RemoteContentBarProps {
  blocked: number;
  /** Address the sender-wide approval would store; null if the mail names no usable sender. */
  sender: string | null;
  pending: RemoteContentScope | null;
  error: IpcError | null;
  onLoad: (scope: RemoteContentScope) => void;
}

/**
 * External content is blocked by default (docs/SPEC.md, "Privat"). This bar says how much and
 * offers to load it once or always for this sender; the core then fetches the images itself,
 * without cookies or referrer.
 */
export function RemoteContentBar({ blocked, sender, pending, error, onLoad }: RemoteContentBarProps) {
  const { t } = useTranslation();
  const titleId = useId();
  return (
    <div
      role="group"
      aria-labelledby={titleId}
      data-testid="remote-content-bar"
      className="flex flex-col gap-3 rounded-card border border-border bg-canvas p-3"
    >
      <div className="flex items-start gap-3">
        <ImageOff aria-hidden className="size-(--ui-icon) shrink-0 text-secondary" strokeWidth={1.75} />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <p id={titleId} className="font-semibold text-primary">
            {t("reading.remote.blocked", { count: blocked })}
          </p>
          <p className="text-13 text-secondary">{t("reading.remote.explanation")}</p>
        </div>
      </div>
      <div className="flex flex-wrap gap-2 ps-8">
        <Button
          variant="secondary"
          disabled={pending !== null}
          onClick={() => {
            onLoad("once");
          }}
          data-testid="remote-once"
        >
          {pending === "once" ? t("reading.remote.loading") : t("reading.remote.once")}
        </Button>
        {sender && (
          <Button
            variant="secondary"
            disabled={pending !== null}
            onClick={() => {
              onLoad("sender");
            }}
            data-testid="remote-sender"
          >
            {pending === "sender" ? t("reading.remote.loading") : t("reading.remote.sender", { sender })}
          </Button>
        )}
      </div>
      {error && <ErrorNotice context={t("reading.remote.failed")} error={error} />}
    </div>
  );
}
