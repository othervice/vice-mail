import { Fragment, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import type { RenderToken, TextSegment } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { LinkCard } from "../reading/LinkCard";

/**
 * Decision 0028: text of an AI result. Text segments are text nodes, never HTML, never an image.
 * A link the core found is an element of its own: activating it asks the core for the real
 * target (`describeLink` with the result's token) and shows the same check as a link in a mail
 * (`AiLinkCheck`); it never navigates by itself. Without a token links stay text.
 */
export function AiText({ segments, linkToken }: { segments: readonly TextSegment[]; linkToken: RenderToken | null }) {
  const requestLink = useUiStore((state) => state.requestLink);
  return (
    <>
      {segments.map((segment, i) =>
        segment.kind === "text" || linkToken === null ? (
          <Fragment key={i}>{segment.text}</Fragment>
        ) : (
          <span
            key={i}
            role="link"
            tabIndex={0}
            onClick={() => {
              requestLink(linkToken, segment.index);
            }}
            onKeyDown={(event: KeyboardEvent) => {
              if (event.key !== "Enter") return;
              event.preventDefault();
              requestLink(linkToken, segment.index);
            }}
            className="cursor-pointer rounded-control text-accent underline underline-offset-2 hover:no-underline"
            data-testid="ai-link"
          >
            {segment.text}
          </span>
        ),
      )}
    </>
  );
}

/**
 * The links of an editable text (a reply suggestion in a text field, where nothing is clickable):
 * listed below it, each opened only through the check.
 */
export function AiLinkList({ segments, linkToken }: { segments: readonly TextSegment[]; linkToken: RenderToken | null }) {
  const { t } = useTranslation();
  const links = segments.flatMap((segment) => (segment.kind === "link" ? [segment] : []));
  if (links.length === 0 || linkToken === null) return null;
  return (
    <div className="flex flex-col gap-1" data-testid="ai-link-list">
      <p className="text-12 text-secondary">{t("ai.links.inText")}</p>
      <ul className="flex flex-col gap-1 text-13 break-all">
        {links.map((link) => (
          <li key={link.index}>
            <AiText segments={[link]} linkToken={linkToken} />
          </li>
        ))}
      </ul>
    </div>
  );
}

/** The check of a link of this result, shown where the result is (the reading pane shows only its own). */
export function AiLinkCheck({ linkToken }: { linkToken: RenderToken | null }) {
  const { t } = useTranslation();
  const link = useUiStore((state) => state.linkRequest);
  if (linkToken === null || link?.renderToken !== linkToken) return null;
  return <LinkCard key={link.seq} request={link} onClosed={() => undefined} staleText={t("ai.links.stale")} />;
}
