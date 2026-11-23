import { act } from "@testing-library/react";
import { appStylesheet, type Stylesheet } from "./styles";

/*
 * Stage 8, "Fokus": every element the keyboard can reach shows the focus ring when it has the
 * focus: a solid outline of at least 2 px in the `focus-ring` token, from the app's compiled
 * stylesheet (tests/support/styles.ts). The ring may also sit on an element around it (a field row
 * that shows the ring while its input is focused) or on the active option of a list that keeps the
 * focus itself (`aria-activedescendant`), as long as it only shows while the focus is there.
 */

const FOCUS_RING = "var(--ui-focus-ring)";

/** Roles that get the focus by arrow keys (roving tabindex), so `tabindex="-1"` is still reachable. */
const ROVING_ROLES = new Set([
  "button",
  "checkbox",
  "gridcell",
  "link",
  "menuitem",
  "menuitemcheckbox",
  "menuitemradio",
  "option",
  "radio",
  "row",
  "switch",
  "tab",
  "treeitem",
]);

const CANDIDATES = "a[href], button, input, select, textarea, summary, iframe, [tabindex], [contenteditable]";

function hiddenByClass(element: Element): boolean {
  for (let node: Element | null = element; node; node = node.parentElement) {
    const classes = node.classList;
    if (classes.contains("hidden") || classes.contains("invisible")) return true;
  }
  return false;
}

/** Elements a keyboard user can move the focus to (and those that ask for a ring themselves). */
export function keyboardFocusable(root: ParentNode = document.body): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(CANDIDATES)].filter((element) => {
    if (element.closest("[hidden], [inert]")) return false;
    if ("disabled" in element && element.disabled === true) return false;
    if (element instanceof HTMLInputElement && element.type === "hidden") return false;
    const editable = element.getAttribute("contenteditable");
    if (editable === "false") return false;
    if (hiddenByClass(element)) return false;
    if (element.tabIndex >= 0 || editable === "true" || editable === "") return true;
    if (element.closest('[aria-hidden="true"]')) return false;
    // `tabindex="-1"`: reachable by arrow keys in a composite widget, or a focus target that
    // asks for a ring itself (a reading pane focused after opening a message).
    return ROVING_ROLES.has(element.getAttribute("role") ?? "") || [...element.classList].some((name) => name.startsWith("focus-visible:"));
  });
}

interface Ring {
  ok: boolean;
  detail: string;
}

function ringOf(sheet: Stylesheet, element: Element): Ring {
  const style = sheet.resolved(element, "outline-style") ?? "none";
  const width = sheet.resolved(element, "outline-width") ?? "medium";
  const color = sheet.resolved(element, "outline-color") ?? "currentcolor";
  const pixels = /^([\d.]+)px$/.exec(width)?.[1];
  const wide = pixels === undefined ? width === "medium" || width === "thick" : Number(pixels) >= 2;
  const ok = style !== "none" && style !== "hidden" && wide && color === FOCUS_RING;
  return { ok, detail: `outline ${width} ${style} ${color}` };
}

function others(element: HTMLElement): Element[] {
  const result: Element[] = [];
  for (let node = element.parentElement; node && node !== document.body; node = node.parentElement) result.push(node);
  const active = element.getAttribute("aria-activedescendant");
  const option = active ? document.getElementById(active) : null;
  if (option) result.push(option, ...option.querySelectorAll("*"));
  return result;
}

export function describeElement(element: Element): string {
  const role = element.getAttribute("role");
  const name =
    element.getAttribute("aria-label") ??
    element.getAttribute("data-testid") ??
    (element.textContent ?? "").replace(/\s+/g, " ").trim().slice(0, 40);
  return `<${element.tagName.toLowerCase()}${role ? ` role="${role}"` : ""}> "${name}"`;
}

export interface FocusReport {
  /** Elements checked (focused one after another). */
  checked: number;
  findings: string[];
}

/**
 * Focuses every keyboard-reachable element under `root` in turn and reports those that show no
 * focus ring from the token. Returns the focus to where it was.
 */
export async function focusRingReport(root: ParentNode = document.body): Promise<FocusReport> {
  const sheet = await appStylesheet(document);
  const before = document.activeElement;
  const findings: string[] = [];
  let checked = 0;
  // Somewhere to put the focus for "not focused" (jsdom does not blur a frame).
  const elsewhere = document.createElement("span");
  elsewhere.tabIndex = -1;
  document.body.append(elsewhere);
  for (const element of keyboardFocusable(root)) {
    if (!element.isConnected) continue;
    // In act(): the focus may change what is shown (the list sets its cursor row).
    act(() => {
      element.focus();
    });
    if (document.activeElement !== element) continue;
    checked += 1;
    const own = ringOf(sheet, element);
    if (own.ok) continue;
    const shown = others(element).filter((other) => ringOf(sheet, other).ok);
    act(() => {
      elsewhere.focus();
    });
    // A ring elsewhere only counts if it comes with the focus, not if it is always there.
    if (shown.some((other) => !ringOf(sheet, other).ok)) continue;
    findings.push(`${describeElement(element)}: ${own.detail}`);
  }
  elsewhere.remove();
  if (before instanceof HTMLElement) {
    act(() => {
      before.focus();
    });
  }
  return { checked, findings };
}
