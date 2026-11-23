/**
 * Stage 16: find in the open message. Searches the text the reading pane shows: the app's own
 * nodes (head, plain-text mail) and the documents of the sandboxed mail frames inside it.
 *
 * The frames are `sandbox="allow-same-origin"` without scripts (decision 0005): nothing runs in
 * them, and under WebKit no event listener on their nodes is ever called. Everything here runs in
 * the app's document and only reads the frame's DOM: it walks the text nodes, builds `Range`s on
 * them and registers them with the CSS Custom Highlight API of the frame's own window (the
 * highlight registry belongs to a document). The highlight colours go into the frame as a
 * `<style>` element made from here (its CSP allows inline styles); no script is ever added and
 * nothing is loaded. Without the API (an older engine, jsdom) the current match is selected in
 * its frame and scrolled into view instead.
 */

export const FIND_HIGHLIGHT = "vice-find";
export const FIND_CURRENT_HIGHLIGHT = "vice-find-current";
/** Marks the style element this module puts into a mail frame. */
const FRAME_STYLE_ATTRIBUTE = "data-vice-find";

/** Elements whose text is never shown. */
const SKIPPED = new Set(["SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "HEAD", "TITLE", "SVG"]);
/** Display values that keep text on the same line as its neighbours. */
const INLINE_DISPLAYS = new Set(["inline", "contents", "inline-block", "inline-flex", ""]);

interface TextRun {
  node: Text;
  start: number;
}

/** The searchable text of one document part, with the text node behind every position. */
export interface FindSegment {
  doc: Document;
  /** The mail frame the text lies in; null for the app's own document. */
  frame: HTMLIFrameElement | null;
  runs: TextRun[];
  text: string;
}

export interface FindMatch {
  segment: FindSegment;
  range: Range;
}

function frameDocument(frame: HTMLIFrameElement): Document | null {
  try {
    return frame.contentDocument;
  } catch {
    // An opaque frame cannot be read: nothing to search in it.
    return null;
  }
}

function computedStyle(element: Element): CSSStyleDeclaration | null {
  try {
    return element.ownerDocument.defaultView?.getComputedStyle(element) ?? null;
  } catch {
    return null;
  }
}

/** Text the user does not see: hidden, screen-reader only, or a part of the app's own controls. */
function hiddenElement(element: Element, inApp: boolean): boolean {
  if (SKIPPED.has(element.tagName.toUpperCase())) return true;
  if (element.hasAttribute("hidden")) return true;
  if (inApp && (element.hasAttribute("data-find-ignore") || element.classList.contains("sr-only"))) return true;
  const style = computedStyle(element);
  return style !== null && (style.display === "none" || style.visibility === "hidden");
}

function isBlock(element: Element): boolean {
  const display = computedStyle(element)?.display ?? "";
  return !INLINE_DISPLAYS.has(display);
}

/**
 * The searchable segments under `root`, in document order: the app's text between frames, and
 * each readable mail frame as a segment of its own. Block boundaries become a line break, so a
 * match never runs from the end of one paragraph into the next.
 */
export function collectSegments(root: Element): FindSegment[] {
  const segments: FindSegment[] = [];
  let current: FindSegment = { doc: root.ownerDocument, frame: null, runs: [], text: "" };

  const close = () => {
    if (current.runs.length > 0) segments.push(current);
  };

  // App text goes to `current`, which starts anew after every frame; frame text to its own.
  const walk = (node: Node, inApp: boolean, frameSegment: FindSegment | null) => {
    for (let child = node.firstChild; child; child = child.nextSibling) {
      const segment = frameSegment ?? current;
      if (child.nodeType === Node.TEXT_NODE) {
        const text = child.nodeValue ?? "";
        if (text.length === 0) continue;
        segment.runs.push({ node: child as Text, start: segment.text.length });
        segment.text += text;
        continue;
      }
      if (child.nodeType !== Node.ELEMENT_NODE) continue;
      const element = child as Element;
      if (inApp && element.tagName.toUpperCase() === "IFRAME") {
        const doc = frameDocument(element as HTMLIFrameElement);
        const body = doc?.body ?? doc?.documentElement;
        if (!doc || !body) continue;
        // The app's text before the frame, the frame's own text, then the app's text after it.
        close();
        const inFrame: FindSegment = { doc, frame: element as HTMLIFrameElement, runs: [], text: "" };
        walk(body, false, inFrame);
        if (inFrame.runs.length > 0) segments.push(inFrame);
        current = { doc: root.ownerDocument, frame: null, runs: [], text: "" };
        continue;
      }
      if (hiddenElement(element, inApp)) continue;
      const block = isBlock(element);
      if (block) (frameSegment ?? current).text += "\n";
      walk(element, inApp, frameSegment);
      if (block) (frameSegment ?? current).text += "\n";
    }
  };

  walk(root, true, null);
  close();
  return segments;
}

/** Lower case that keeps every position where it was (a character that would grow stays as it is). */
function fold(text: string): string {
  let folded = "";
  for (const char of text) {
    const lower = char.toLocaleLowerCase();
    folded += lower.length === char.length ? lower : char;
  }
  return folded;
}

/** The text run that holds `position` (the last one starting at or before it). */
function runAt(runs: readonly TextRun[], position: number): TextRun | undefined {
  let low = 0;
  let high = runs.length - 1;
  let found: TextRun | undefined;
  while (low <= high) {
    const middle = (low + high) >> 1;
    const run = runs[middle];
    if (!run) break;
    if (run.start <= position) {
      found = run;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return found;
}

/** Every match of `query` (case ignored, not overlapping), in document order. */
export function findMatches(segments: readonly FindSegment[], query: string): FindMatch[] {
  if (query.trim() === "") return [];
  const needle = fold(query);
  const matches: FindMatch[] = [];
  for (const segment of segments) {
    const haystack = fold(segment.text);
    let from = 0;
    for (;;) {
      const start = haystack.indexOf(needle, from);
      if (start < 0) break;
      const end = start + needle.length;
      from = end;
      const first = runAt(segment.runs, start);
      const last = runAt(segment.runs, end - 1);
      if (!first || !last) continue;
      const range = segment.doc.createRange();
      range.setStart(first.node, start - first.start);
      range.setEnd(last.node, end - last.start);
      matches.push({ segment, range });
    }
  }
  return matches;
}

interface HighlightWindow {
  CSS?: { highlights?: Map<string, unknown> };
  Highlight?: new (...ranges: Range[]) => unknown;
}

function highlightApi(doc: Document): { registry: Map<string, unknown>; make: (ranges: Range[]) => unknown } | null {
  const view = doc.defaultView as (Window & HighlightWindow) | null;
  const registry = view?.CSS?.highlights;
  const Highlight = view?.Highlight;
  if (!registry || typeof Highlight !== "function") return null;
  return { registry, make: (ranges) => new Highlight(...ranges) };
}

/** The highlight colours of the app's tokens, for a mail frame (it cannot read the app's variables). */
function frameStyleText(): string | null {
  const style = getComputedStyle(document.documentElement);
  const match = style.getPropertyValue("--ui-mail-find-match").trim();
  const current = style.getPropertyValue("--ui-mail-find-current").trim();
  if (!match || !current) return null;
  return (
    `::highlight(${FIND_HIGHLIGHT}){background-color:${match};}` +
    `::highlight(${FIND_CURRENT_HIGHLIGHT}){background-color:${current};text-decoration:underline 2px;}`
  );
}

function ensureFrameStyle(doc: Document): void {
  if (doc.querySelector(`style[${FRAME_STYLE_ATTRIBUTE}]`)) return;
  const text = frameStyleText();
  if (!text) return;
  const style = doc.createElement("style");
  style.setAttribute(FRAME_STYLE_ATTRIBUTE, "");
  style.textContent = text;
  (doc.head ?? doc.documentElement).appendChild(style);
}

/** Removes the highlights (and the frames' style elements) from every document in `docs`. */
export function clearHighlights(docs: Set<Document>): void {
  for (const doc of docs) {
    const api = highlightApi(doc);
    api?.registry.delete(FIND_HIGHLIGHT);
    api?.registry.delete(FIND_CURRENT_HIGHLIGHT);
    if (doc !== document) doc.querySelector(`style[${FRAME_STYLE_ATTRIBUTE}]`)?.remove();
  }
  docs.clear();
}

/**
 * Shows all matches and the current one. `docs` collects every document touched, so a later
 * call (or `clearHighlights`) can take the highlights away again.
 */
export function paintMatches(matches: readonly FindMatch[], current: number, docs: Set<Document>): void {
  const byDoc = new Map<Document, FindMatch[]>();
  for (const match of matches) byDoc.set(match.segment.doc, [...(byDoc.get(match.segment.doc) ?? []), match]);
  // Documents that no longer have matches lose their highlights.
  for (const doc of [...docs]) {
    if (byDoc.has(doc)) continue;
    clearHighlights(new Set([doc]));
    docs.delete(doc);
  }
  const currentMatch = matches[current];
  for (const [doc, list] of byDoc) {
    docs.add(doc);
    const api = highlightApi(doc);
    if (api) {
      if (doc !== document) ensureFrameStyle(doc);
      api.registry.set(FIND_HIGHLIGHT, api.make(list.filter((match) => match !== currentMatch).map((match) => match.range)));
      api.registry.set(FIND_CURRENT_HIGHLIGHT, api.make(currentMatch && list.includes(currentMatch) ? [currentMatch.range] : []));
    } else if (currentMatch && list.includes(currentMatch) && doc !== document) {
      // Fallback: select the current match in its frame (the find field keeps the app's focus).
      const selection = doc.defaultView?.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(currentMatch.range);
    }
  }
}

/** Scrolls the pane so the match is in view (a third from the top), if it is not already. */
export function scrollToMatch(match: FindMatch, scroller: HTMLElement): void {
  if (typeof match.range.getBoundingClientRect !== "function") return;
  const rect = match.range.getBoundingClientRect();
  const offset = match.segment.frame?.getBoundingClientRect().top ?? 0;
  const top = rect.top + offset;
  const bottom = rect.bottom + offset;
  const pane = scroller.getBoundingClientRect();
  const margin = 24;
  if (top >= pane.top + margin && bottom <= pane.bottom - margin) return;
  scroller.scrollTop += top - pane.top - pane.height / 3;
}
