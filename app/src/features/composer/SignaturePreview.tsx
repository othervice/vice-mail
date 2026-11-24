import { Fragment, type ReactNode } from "react";
import { cx } from "../../components/cx";

/** How deep the preview follows nested elements; a signature never needs more. */
const MAX_DEPTH = 24;

/** `text-align` of a block, as the editor writes it (`style="text-align: center"`). */
const ALIGN: Record<string, string> = { center: "text-center", right: "text-right", justify: "text-justify" };

function alignClass(element: Element): string | undefined {
  const match = /text-align:\s*(center|right|justify)/i.exec(element.getAttribute("style") ?? "");
  return match?.[1] ? ALIGN[match[1].toLowerCase()] : undefined;
}

function children(node: Node, depth: number): ReactNode[] {
  return [...node.childNodes].map((child, index) => <Fragment key={index}>{render(child, depth + 1)}</Fragment>);
}

/**
 * One node as React elements. Only the editor's basic formatting is kept (paragraphs, line breaks,
 * bold, italic, underline, strikethrough, code, lists, quotes, headings, alignment); a link shows
 * as styled text, an image is left out, anything else becomes its text. No attribute of the HTML
 * reaches the page.
 */
function render(node: Node, depth: number): ReactNode {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent;
  if (node.nodeType !== Node.ELEMENT_NODE || depth > MAX_DEPTH) return null;
  const element = node as Element;
  const inner = children(element, depth);
  switch (element.localName) {
    case "p":
    case "div":
      return <p className={cx("min-h-5", alignClass(element))}>{inner}</p>;
    case "h1":
    case "h2":
    case "h3":
      return <p className={cx("font-semibold", alignClass(element))}>{inner}</p>;
    case "br":
      return <br />;
    case "strong":
    case "b":
      return <strong className="font-semibold">{inner}</strong>;
    case "em":
    case "i":
      return <em>{inner}</em>;
    case "u":
      return <u>{inner}</u>;
    case "s":
    case "strike":
    case "del":
      return <s>{inner}</s>;
    case "code":
      return <code className="font-mono">{inner}</code>;
    case "a":
      return <span className="text-accent underline">{inner}</span>;
    case "ul":
      return <ul className="list-disc ps-6">{inner}</ul>;
    case "ol":
      return <ol className="list-decimal ps-6">{inner}</ol>;
    case "li":
      return <li>{inner}</li>;
    case "blockquote":
      return <blockquote className="border-s-2 border-border-strong ps-3">{inner}</blockquote>;
    case "img":
    case "script":
    case "style":
    case "template":
      return null;
    default:
      return inner;
  }
}

/**
 * The HTML of a signature as the composer shows it under the text: its formatting, read from the
 * allowlisted editor HTML the core keeps (`mail_content::mail_html`, docs/ARCHITECTURE.md,
 * "HTML im Composer"). Parsed with `DOMParser`, whose document runs no script and loads nothing,
 * and rebuilt as React elements from a short list, so the page never receives raw HTML.
 */
export function SignatureHtml({ html }: { html: string }) {
  const document = new DOMParser().parseFromString(html, "text/html");
  return <>{children(document.body, 0)}</>;
}
