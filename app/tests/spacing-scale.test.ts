import { compile } from "tailwindcss";
import tailwindIndex from "tailwindcss/index.css?raw";
import ts from "typescript";
import { describe, expect, it } from "vitest";
import appCss from "../src/styles/app.css?raw";
import tokensCss from "../src/styles/tokens.css?raw";

// Stage 21, item 4 (decision 0071): the theme in app.css holds only the steps of the design
// tokens: spacing on the 4 px grid (0, 1, 2, 3, 4, 5, 6, 8, 10, 12), the font sizes of
// docs/DESIGN.md ("Tokens") and the four named radii. A utility with any other step, such as
// `gap-1.5`, `h-7`, `text-15` or a bare `rounded`, makes Tailwind generate no CSS at all, silently:
// the element simply loses its gap, height or radius. This test asks Tailwind itself, with the
// app's own style sheet, which of the utilities written in the code produce CSS, so the scale has
// one source (app.css) and the test cannot drift from it.
//
// Checked: every string literal and template text in src (comments are not code), every class
// whose utility is a spacing, sizing or inset one with a number as its value, a font size
// (`text-<number>` or a size name like `text-sm`) or a radius (`rounded`, `rounded-<name>`).
// Variants (`hover:`, `data-[x]:`, `max-[1100px]:`) and `!` are stripped first. Arbitrary values
// (`p-[3px]`, `w-(--ui-avatar)`) are out of scope: they always produce CSS.

const sources = import.meta.glob<string>(["../src/**/*.{ts,tsx}", "!../src/generated/**"], {
  query: "?raw",
  import: "default",
  eager: true,
});

/** Spacing, sizing and inset utilities; Tailwind resolves their numeric values on `--spacing-*`. */
const SPACING_ROOTS = [
  "p",
  "px",
  "py",
  "pt",
  "pr",
  "pb",
  "pl",
  "ps",
  "pe",
  "m",
  "mx",
  "my",
  "mt",
  "mr",
  "mb",
  "ml",
  "ms",
  "me",
  "gap",
  "gap-x",
  "gap-y",
  "space-x",
  "space-y",
  "inset",
  "inset-x",
  "inset-y",
  "top",
  "right",
  "bottom",
  "left",
  "start",
  "end",
  "w",
  "h",
  "size",
  "min-w",
  "min-h",
  "max-w",
  "max-h",
  "translate-x",
  "translate-y",
  "scroll-m",
  "scroll-mx",
  "scroll-my",
  "scroll-mt",
  "scroll-mb",
  "scroll-ms",
  "scroll-me",
  "scroll-p",
  "scroll-px",
  "scroll-py",
  "scroll-pt",
  "scroll-pb",
  "scroll-ps",
  "scroll-pe",
  "indent",
  "basis",
  "leading",
];
const SPACING = new RegExp(`^-?(?:${SPACING_ROOTS.join("|")})-\\d`);
const FONT_SIZE = /^text-(?:\d|xs$|sm$|base$|lg$|[2-9]?xl$)/;
const RADIUS = /^rounded(?:-(?:[trbles]|tl|tr|br|bl|ss|se|es|ee))?(?:-[a-z0-9]+)?$/;

/** The utility part of a class: without its variants (`hover:`, `data-[x]:`) and `!`. */
function utility(candidate: string): string {
  let depth = 0;
  let start = 0;
  for (let i = 0; i < candidate.length; i += 1) {
    const c = candidate[i];
    if (c === "[" || c === "(") depth += 1;
    else if (c === "]" || c === ")") depth -= 1;
    else if (c === ":" && depth === 0) start = i + 1;
  }
  return candidate.slice(start).replace(/^!|!$/g, "");
}

type Use = { utility: string; where: string };

/** Every class of a family above written in a string literal or template text of the sources. */
function uses(): Use[] {
  const found: Use[] = [];
  for (const [file, text] of Object.entries(sources)) {
    const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, file.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
    const visit = (node: ts.Node) => {
      if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node) || ts.isTemplateLiteralToken(node)) {
        const start = node.getStart(source);
        // Line by line, so a hit in a long template names its own line.
        node.text.split("\n").forEach((line, offset) => {
          for (const word of line.split(/\s+/)) {
            const candidate = utility(word);
            if (SPACING.test(candidate) || FONT_SIZE.test(candidate) || RADIUS.test(candidate)) {
              const lineNumber = source.getLineAndCharacterOfPosition(start).line + 1 + offset;
              found.push({ utility: candidate, where: `${file.replace("../", "")}:${String(lineNumber)}` });
            }
          }
        });
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
  }
  return found;
}

/** Tailwind with the app's style sheet, as the Vite plugin compiles it. */
async function tailwind() {
  return compile(appCss, {
    base: "/",
    loadStylesheet: (id: string) => {
      if (id === "tailwindcss") return Promise.resolve({ path: "tailwindcss/index.css", base: "/", content: tailwindIndex });
      if (id === "./tokens.css") return Promise.resolve({ path: "tokens.css", base: "/", content: tokensCss });
      return Promise.reject(new Error(`unexpected @import ${id}`));
    },
  });
}

/** Whether the CSS has a rule for the class `name` (`gap-1.5` has the selector `.gap-1\.5`). */
function hasRule(css: string, name: string): boolean {
  const selector = `.${name.replace(/[^a-zA-Z0-9_-]/g, (c) => `\\${c}`)}`;
  for (let at = css.indexOf(selector); at !== -1; at = css.indexOf(selector, at + 1)) {
    if (!/[\w\\-]/.test(css[at + selector.length] ?? "")) return true;
  }
  return false;
}

describe("the spacing, type and radius scale", () => {
  it("finds the utilities it checks, and tells steps of the scale from steps between them", async () => {
    const found = uses();
    // A broken extraction or check would make the test below pass on nothing.
    expect(Object.keys(sources).length).toBeGreaterThan(100);
    expect(found.length).toBeGreaterThan(1000);
    for (const sample of ["gap-2", "px-4", "text-13", "rounded-control", "size-4"]) {
      expect(found.some((u) => u.utility === sample)).toBe(true);
    }
    // Stage 23 (decision 0083): 11 and 26 joined the type scale, 15 is still no step of it.
    const css = (await tailwind()).build(["gap-2", "gap-1.5", "-mt-1", "w-1/2", "text-13", "text-11", "text-26", "text-15", "rounded-card", "rounded-tile", "rounded"]);
    expect(["gap-2", "-mt-1", "w-1/2", "text-13", "text-11", "text-26", "rounded-card", "rounded-tile"].filter((name) => !hasRule(css, name))).toEqual([]);
    expect(["gap-1.5", "text-15", "rounded"].filter((name) => hasRule(css, name))).toEqual([]);
  });

  it("Tailwind generates CSS for every spacing, size, font size and radius in the code", async () => {
    const found = uses();
    const css = (await tailwind()).build([...new Set(found.map((u) => u.utility))]);
    const off = found.filter((u) => !hasRule(css, u.utility)).map((u) => `${u.where}: ${u.utility}`);
    expect(off, "utilities without CSS: use a step of the scale (app.css, @theme) or a token").toEqual([]);
  });
});
