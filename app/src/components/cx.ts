/** Joins class names, skipping falsy parts: `cx("a", active && "b")`. */
export function cx(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}
