// Decision 0031: list options are named by their content (WCAG 2.5.3), not by `aria-label`.

/**
 * The accessible name of an option computed from its content, as screen readers get it: the text
 * of every descendant that is neither `aria-hidden` nor `hidden`. Message rows hold their name in
 * one visually hidden text; drawn rows with an action (bundles, "Show all") are named by what they
 * show plus visually hidden separators and state.
 */
export function optionName(element: Element | null | undefined): string {
  if (!element) return "";
  let text = "";
  for (const node of element.childNodes) {
    if (node.nodeType === Node.TEXT_NODE) text += node.textContent ?? "";
    else if (node instanceof Element && node.getAttribute("aria-hidden") !== "true" && !node.hasAttribute("hidden")) {
      text += optionName(node);
    }
  }
  return text;
}
