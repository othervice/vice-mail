import type { Address } from "../../services/ipc";

/**
 * A rough check so the user sees a typo at once. The core checks every address properly when
 * the message is sent (`invalid_address`); this only catches what can never be an address.
 */
export function looksLikeAddress(email: string): boolean {
  return /^[^\s@<>(),;:"]+@[^\s@<>(),;:"]+$/.test(email) && !email.endsWith(".") && !email.includes("..");
}

/** One typed or pasted entry: `Name <a@b>`, `"Last, First" <a@b>`, `<a@b>` or `a@b`. */
export function parseAddress(entry: string): Address | null {
  const text = entry.trim();
  if (text === "") return null;
  const angle = /^(.*?)<\s*([^<>]*?)\s*>\s*$/.exec(text);
  if (angle) {
    const rawName = (angle[1] ?? "").trim().replace(/^"(.*)"$/, "$1").trim();
    return { name: rawName === "" ? null : rawName, email: (angle[2] ?? "").trim() };
  }
  return { name: null, email: text.replace(/^mailto:/i, "") };
}

/**
 * Splits pasted or typed text into entries at commas, semicolons and line breaks, but not
 * inside quotes or angle brackets (`"Lovelace, Ada" <ada@example.org>` is one entry).
 */
export function splitAddressList(text: string): string[] {
  const parts: string[] = [];
  let current = "";
  let quoted = false;
  let angle = false;
  for (const char of text) {
    if (char === '"' && !angle) quoted = !quoted;
    else if (char === "<" && !quoted) angle = true;
    else if (char === ">" && !quoted) angle = false;
    if (!quoted && !angle && (char === "," || char === ";" || char === "\n" || char === "\r")) {
      parts.push(current);
      current = "";
      continue;
    }
    current += char;
  }
  parts.push(current);
  return parts.map((part) => part.trim()).filter((part) => part !== "");
}

export function parseAddressList(text: string): Address[] {
  return splitAddressList(text).flatMap((entry) => parseAddress(entry) ?? []);
}

/** Same mailbox: addresses compare without case. */
export function sameAddress(a: Address, b: Address): boolean {
  return a.email.toLowerCase() === b.email.toLowerCase();
}

/** Adds addresses that are not in the list yet. */
export function addAddresses(list: readonly Address[], added: readonly Address[]): Address[] {
  const next = [...list];
  for (const address of added) if (!next.some((a) => sameAddress(a, address))) next.push(address);
  return next;
}

/** How a chip names an address: the name if there is one, else the address. */
export function addressLabel(address: Address): string {
  return address.name?.trim() ? address.name : address.email;
}

/** `Name <a@b>` for tooltips and the screen reader. */
export function addressFull(address: Address): string {
  return address.name?.trim() ? `${address.name} <${address.email}>` : address.email;
}

/** The address the core named in an `invalid_address` detail, if it is one of these. */
export function invalidAddressIn(detail: string | null, lists: readonly (readonly Address[])[]): string | null {
  if (!detail) return null;
  const lower = detail.toLowerCase();
  for (const list of lists) {
    for (const address of list) {
      if (address.email !== "" && lower.includes(address.email.toLowerCase())) return address.email;
    }
  }
  return null;
}

/** File sizes as the user reads them: "820 KB", "3.4 MB". */
export function formatSize(bytes: number, locale: string): string {
  const units = ["byte", "kilobyte", "megabyte", "gigabyte"] as const;
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  return new Intl.NumberFormat(locale, {
    style: "unit",
    unit: units[unit],
    unitDisplay: unit === 0 ? "long" : "short",
    maximumFractionDigits: unit < 2 ? 0 : 1,
  }).format(value);
}

/**
 * Plain text from the editor's HTML (switching to plain text): blocks become lines, list items
 * get a dash, quotes a "> ". Parsed inert (DOMParser runs no scripts and loads nothing).
 */
export function htmlToText(html: string): string {
  if (html.trim() === "") return "";
  const doc = new DOMParser().parseFromString(html, "text/html");
  const lines: string[] = [];
  const walk = (node: Node, prefix: string): void => {
    for (const child of node.childNodes) {
      if (!(child instanceof HTMLElement)) continue;
      const tag = child.tagName.toLowerCase();
      if (tag === "ul" || tag === "ol") {
        let n = 1;
        for (const item of child.children) {
          const marker = tag === "ol" ? `${String(n++)}. ` : "- ";
          lines.push(prefix + marker + inlineText(item));
        }
      } else if (tag === "blockquote") {
        walk(child, `${prefix}> `);
      } else {
        lines.push(prefix + inlineText(child));
      }
    }
  };
  walk(doc.body, "");
  return lines.join("\n").replace(/\n{3,}/g, "\n\n").trim();
}

function inlineText(element: Element): string {
  let text = "";
  for (const node of element.childNodes) {
    if (node instanceof HTMLBRElement) text += "\n";
    else text += node.textContent;
  }
  return text;
}

const escapeHtml = (text: string) =>
  text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/** Editor HTML from plain text (switching to rich text): paragraphs at blank lines, `<br>` inside. */
export function textToHtml(text: string): string {
  if (text.trim() === "") return "";
  return text
    .replace(/\r\n?/g, "\n")
    .split(/\n{2,}/)
    .map((paragraph) => `<p>${escapeHtml(paragraph).replace(/\n/g, "<br>")}</p>`)
    .join("");
}
