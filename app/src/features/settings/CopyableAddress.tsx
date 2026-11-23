import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";

interface CopyableAddressProps {
  /** The full address that is copied. */
  url: string;
  /** A fuller name for the button where several of them can be on the page. */
  copyLabel?: string;
  "data-testid"?: string;
}

/**
 * A web address as selectable text and a button that copies it. The app opens no web pages
 * itself (the frontend has no network and no command for it), so it is not a link.
 */
export function CopyableAddress({ url, copyLabel, ...rest }: CopyableAddressProps) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState<"copied" | "failed" | null>(null);
  const address = url.replace(/^https:\/\//, "");
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
      <span className="font-mono text-13 break-all select-text">{address}</span>
      <Button
        variant="quiet"
        {...(copyLabel ? { "aria-label": copyLabel } : {})}
        {...(rest["data-testid"] ? { "data-testid": rest["data-testid"] } : {})}
        onClick={() => {
          const clipboard = typeof navigator === "undefined" ? undefined : (navigator.clipboard as Clipboard | undefined);
          if (!clipboard) {
            setCopied("failed");
            return;
          }
          clipboard.writeText(url).then(
            () => {
              setCopied("copied");
            },
            () => {
              setCopied("failed");
            },
          );
        }}
      >
        {t("settings.about.copy")}
      </Button>
      <span role="status" className={copied === "failed" ? "text-12 text-danger" : "text-12 text-secondary"}>
        {copied && t(copied === "copied" ? "settings.about.copied" : "settings.about.copyFailed")}
      </span>
    </div>
  );
}
