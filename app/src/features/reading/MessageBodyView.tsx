import { Fragment, useRef, type KeyboardEvent, type RefObject } from "react";
import type { RenderToken, TextSegment } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { useFrameHeight } from "./frameHeight";

/**
 * The only sandbox value the app ever gives a mail frame (docs/ARCHITECTURE.md, security
 * boundary 3; decision 0005): the app may read the frame's document (height), and nothing in it
 * runs. Never add `allow-scripts`: together with this token a script would reach the app's IPC.
 */
export const MAIL_FRAME_SANDBOX = "allow-same-origin";

interface HtmlBodyProps {
  /** From the core, unchanged: it starts with the mandatory meta CSP and is sanitized. */
  srcdoc: string;
  title: string;
  scrollerRef: RefObject<HTMLElement | null>;
}

/**
 * HTML mail on its own white canvas, in the dark theme too (docs/DESIGN.md). Clicks on links
 * never reach this code: the core's navigation hook sees them and sends `linkActivated`.
 */
export function HtmlBody({ srcdoc, title, scrollerRef }: HtmlBodyProps) {
  const frameRef = useRef<HTMLIFrameElement | null>(null);
  useFrameHeight(frameRef, scrollerRef);
  return (
    // Stage 8: the frame's own focus ring would be cut off by the rounded clip, so the card shows
    // it while the focus is in the mail (`:focus-within` reaches into the frame, `:focus-visible`
    // of the frame element is not the same in WebKit and WebView2).
    // No frame of its own: in the light theme the mail's white canvas is the card's surface; in
    // the dark theme it is a white sheet inside the card.
    <div className="overflow-hidden rounded-card bg-mail-canvas focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-focus-ring">
      <iframe
        ref={frameRef}
        sandbox={MAIL_FRAME_SANDBOX}
        srcDoc={srcdoc}
        title={title}
        referrerPolicy="no-referrer"
        data-testid="mail-frame"
        // Light scheme: the mail's default text is black on the white canvas in both themes.
        className="block h-0 w-full border-0 bg-mail-canvas scheme-light"
      />
    </div>
  );
}

interface TextBodyProps {
  segments: readonly TextSegment[];
  renderToken: RenderToken;
  label: string;
}

/**
 * A plain-text mail in the app's own theme. The text is rendered as text nodes, never as HTML.
 * Links found by the core are separate segments: activating one shows the same check as a link
 * in an HTML mail.
 */
export function TextBody({ segments, renderToken, label }: TextBodyProps) {
  const requestLink = useUiStore((state) => state.requestLink);
  const activate = (index: number) => {
    requestLink(renderToken, index);
  };
  return (
    <div
      role="document"
      aria-label={label}
      data-testid="text-body"
      className="max-w-(--ui-reading-max-width) text-14 leading-(--ui-reading-line-height) break-words whitespace-pre-wrap text-primary select-text"
    >
      {segments.map((segment, i) =>
        segment.kind === "text" ? (
          <Fragment key={i}>{segment.text}</Fragment>
        ) : (
          <span
            key={i}
            role="link"
            tabIndex={0}
            onClick={() => {
              activate(segment.index);
            }}
            onKeyDown={(event: KeyboardEvent) => {
              if (event.key !== "Enter") return;
              event.preventDefault();
              activate(segment.index);
            }}
            className="cursor-pointer rounded-control text-accent underline underline-offset-2 hover:no-underline"
          >
            {segment.text}
          </span>
        ),
      )}
    </div>
  );
}
