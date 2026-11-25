import { Palette } from "lucide-react";
import { Fragment, useRef, type KeyboardEvent, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import type { MailColors, MailScheme, RenderToken, TextSegment } from "../../services/ipc";
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
  /** Stage 21 (decision 0066): how the core showed the mail's colours. */
  colors: MailColors;
}

/**
 * HTML mail. Shown as written it lies on its own white canvas, in the dark theme too; adapted
 * to the dark theme (or with dark colours of its own) the core painted it on the dark card's
 * surface, so frame and card are one (docs/DESIGN.md, decision 0066). Clicks on links never
 * reach this code: the core's navigation hook sees them and sends `linkActivated`.
 */
export function HtmlBody({ srcdoc, title, scrollerRef, colors }: HtmlBodyProps) {
  const frameRef = useRef<HTMLIFrameElement | null>(null);
  useFrameHeight(frameRef, scrollerRef);
  const dark = colors !== "original";
  return (
    // Stage 8: the frame's own focus ring would be cut off by the rounded clip, so the card shows
    // it while the focus is in the mail (`:focus-within` reaches into the frame, `:focus-visible`
    // of the frame element is not the same in WebKit and WebView2).
    // No frame of its own: in the light theme the mail's white canvas is the card's surface; in
    // the dark theme it is a white sheet inside the card, or the card itself when adapted.
    <div
      className={cx(
        "overflow-hidden rounded-card focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-focus-ring",
        dark ? "bg-surface-raised" : "bg-mail-canvas",
      )}
    >
      <iframe
        ref={frameRef}
        sandbox={MAIL_FRAME_SANDBOX}
        srcDoc={srcdoc}
        title={title}
        referrerPolicy="no-referrer"
        data-testid="mail-frame"
        data-colors={colors}
        // The frame's scheme matches its document's (the core sets it): light for the white
        // canvas, dark for the adapted mail.
        className={cx("block h-0 w-full border-0", dark ? "bg-surface-raised scheme-dark" : "bg-mail-canvas scheme-light")}
      />
    </div>
  );
}

/** Stage 21 (decision 0066): the scheme to ask the core for, from the window's theme and the toggle. */
export function mailScheme(theme: "light" | "dark", original: boolean): MailScheme {
  return theme === "dark" && !original ? "dark" : "light";
}

interface ColorsToggleProps {
  /** "Original colours" is on: the mail as written, on its white canvas. */
  original: boolean;
  onChange: (original: boolean) => void;
}

/**
 * Stage 21 (decision 0066): in the dark theme, one message as the sender designed it (on its
 * white canvas) or adapted again. A toggle button beside the HTML/text switch; it belongs to its
 * message, the next one is adapted again.
 */
export function ColorsToggle({ original, onChange }: ColorsToggleProps) {
  const { t } = useTranslation();
  return (
    <div className="inline-flex shrink-0 rounded-card bg-hover p-1">
      <button
        type="button"
        aria-pressed={original}
        title={original ? t("reading.colors.adaptedHint") : t("reading.colors.originalHint")}
        onClick={() => {
          onChange(!original);
        }}
        data-testid="colors-original"
        className="inline-flex h-6 items-center gap-1 rounded-control px-2 text-12 font-medium text-secondary transition-colors hover:text-primary aria-pressed:bg-surface-raised aria-pressed:text-primary aria-pressed:shadow-row"
      >
        <Palette aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
        {t("reading.colors.original")}
      </button>
    </div>
  );
}

/**
 * Whether `frame` shows the rendering with `colors`, loaded: the attribute changes with the new
 * `srcdoc` at once, the document only once it has loaded, so the document's own default style is
 * checked too (the dark one names `color-scheme:dark`).
 */
function frameReady(frame: HTMLIFrameElement | null | undefined, colors: MailColors): boolean {
  try {
    const doc = frame?.contentDocument;
    if (frame?.dataset["colors"] !== colors || doc?.readyState !== "complete") return false;
    const dark = [...doc.querySelectorAll("style")].some((style) => (style.textContent ?? "").includes("color-scheme:dark"));
    return dark === (colors !== "original");
  } catch {
    return false;
  }
}

/**
 * Resolves once the mail frame in `root` shows the colours `colors` and its document has loaded
 * (and been measured), or after `timeoutMs`. Printing waits for the original colours this way:
 * paper is white (decision 0066).
 */
export function whenFrameShows(root: HTMLElement | null, colors: MailColors, timeoutMs = 3000): Promise<void> {
  const started = Date.now();
  return new Promise((resolve) => {
    const check = () => {
      const frame = root?.querySelector<HTMLIFrameElement>('[data-testid="mail-frame"]');
      const ready = frameReady(frame, colors);
      if (ready || Date.now() - started >= timeoutMs) {
        // One more task for the frame's height (measured on load).
        window.setTimeout(resolve, ready ? 50 : 0);
        return;
      }
      window.setTimeout(check, 30);
    };
    check();
  });
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
