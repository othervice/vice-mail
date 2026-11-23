import { Fragment } from "react";

/** A range of UTF-16 code units (`start..end`, end exclusive), as the core sends them. */
export interface TextSpan {
  start: number;
  end: number;
}

/**
 * Splits `text` into plain and marked pieces. `offset` is where `text` starts in the string the
 * ranges refer to (the caller trimmed it). Ranges outside the text, empty, overlapping or out of
 * order are clipped and merged, so a wrong range can never cut a character in two or repeat text.
 */
export function splitHighlights(
  text: string,
  ranges: readonly TextSpan[],
  offset = 0,
): { text: string; marked: boolean }[] {
  const spans = ranges
    .map((range) => ({ start: Math.max(0, range.start - offset), end: Math.min(text.length, range.end - offset) }))
    .filter((range) => range.end > range.start)
    .sort((a, b) => a.start - b.start);
  const pieces: { text: string; marked: boolean }[] = [];
  let at = 0;
  for (const span of spans) {
    const start = Math.max(span.start, at);
    if (span.end <= start) continue;
    if (start > at) pieces.push({ text: text.slice(at, start), marked: false });
    pieces.push({ text: text.slice(start, span.end), marked: true });
    at = span.end;
  }
  if (at < text.length) pieces.push({ text: text.slice(at), marked: false });
  return pieces;
}

/** Text with the found places marked (`<mark>`, which screen readers can announce). */
export function HighlightedText({ text, ranges, offset = 0 }: { text: string; ranges: readonly TextSpan[]; offset?: number }) {
  if (ranges.length === 0) return text;
  return splitHighlights(text, ranges, offset).map((piece, index) =>
    piece.marked ? (
      <mark key={index} className="bg-accent-soft text-inherit" data-testid="highlight">
        {piece.text}
      </mark>
    ) : (
      <Fragment key={index}>{piece.text}</Fragment>
    ),
  );
}
