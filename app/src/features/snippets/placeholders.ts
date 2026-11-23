// Stage 10: the placeholders templates may use (decision 0023). The core fills them in
// `fillTemplate`; the UI only offers them for insertion and names them.

/** Known placeholders, in the order the template editor offers them (`snippets.placeholders.*`). */
export const TEMPLATE_PLACEHOLDERS = [
  "first_name",
  "last_name",
  "name",
  "email",
  "date",
  "my_name",
  "my_first_name",
  "my_email",
] as const;

export type TemplatePlaceholder = (typeof TEMPLATE_PLACEHOLDERS)[number];

/** How a placeholder is written in a template: `{{first_name}}`. */
export function placeholderToken(name: string): string {
  return `{{${name}}}`;
}

/** Escapes text for editor HTML (a placeholder token has no markup, but stay safe). */
export function escapeHtml(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

/** Removes the `<img>` of one `cid:` image from editor HTML (after the image was removed). */
export function withoutImage(html: string, contentId: string): string {
  const escaped = contentId.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return html.replace(new RegExp(`<img\\b[^>]*src="cid:${escaped}"[^>]*>`, "g"), "");
}
