import { compile } from "tailwindcss";
import tailwindIndexCss from "tailwindcss/index.css?raw";
import appCss from "../../src/styles/app.css?raw";
import tokensCss from "../../src/styles/tokens.css?raw";
import composerCss from "../../src/features/composer/composer.css?raw";

/*
 * Stage 8: the app's real stylesheet in jsdom. jsdom loads no CSS and has no cascade worth the
 * name, so this compiles src/styles/app.css with Tailwind (the same compiler the Vite plugin uses)
 * for exactly the classes on the page, adds the app's plain stylesheets, and resolves a handful of
 * properties for an element in its current state (focus, `:focus-visible`, `:has`, group variants
 * come from jsdom's selector engine). Enough to tell which outline a focused element really gets,
 * and which transition it runs; no layout, no inheritance.
 */

interface Declaration {
  property: string;
  value: string;
  important: boolean;
}

interface Rule {
  selectors: string[];
  declarations: Declaration[];
  /** Cascade layer rank: unlayered styles beat every layer. */
  layer: number;
  media: string[];
  order: number;
}

type Node = { kind: "text"; text: string } | { kind: "block"; prelude: string; children: Node[] };

const LAYERS: Record<string, number> = { properties: 0, theme: 1, base: 2, components: 3, utilities: 4 };
const UNLAYERED = 100;

function parse(source: string): Node[] {
  const css = source.replace(/\/\*[\s\S]*?\*\//g, "");
  let index = 0;
  const list = (): Node[] => {
    const nodes: Node[] = [];
    let buffer = "";
    const flush = () => {
      if (buffer.trim()) nodes.push({ kind: "text", text: buffer.trim() });
      buffer = "";
    };
    while (index < css.length) {
      const char = css.charAt(index);
      if (char === '"' || char === "'") {
        const end = css.indexOf(char, index + 1);
        buffer += css.slice(index, end + 1);
        index = end + 1;
      } else if (char === "{") {
        index += 1;
        const prelude = buffer.trim();
        buffer = "";
        nodes.push({ kind: "block", prelude, children: list() });
      } else if (char === "}") {
        index += 1;
        flush();
        return nodes;
      } else if (char === ";") {
        index += 1;
        flush();
      } else {
        buffer += char;
        index += 1;
      }
    }
    flush();
    return nodes;
  };
  return list();
}

/** Splits at top-level commas (not inside parentheses or brackets). */
function splitList(text: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let current = "";
  for (const char of text) {
    if (char === "(" || char === "[") depth += 1;
    if (char === ")" || char === "]") depth -= 1;
    if (char === "," && depth === 0) {
      parts.push(current.trim());
      current = "";
    } else current += char;
  }
  if (current.trim()) parts.push(current.trim());
  return parts;
}

function nest(parents: string[] | null, selectors: string[]): string[] {
  if (!parents) return selectors;
  return parents.flatMap((parent) =>
    selectors.map((selector) => (selector.includes("&") ? selector.replaceAll("&", `:is(${parent})`) : `${parent} ${selector}`)),
  );
}

function declaration(text: string): Declaration | null {
  const colon = text.indexOf(":");
  if (colon < 0) return null;
  const property = text.slice(0, colon).trim();
  let value = text.slice(colon + 1).trim();
  const important = /!important$/.test(value);
  if (important) value = value.replace(/\s*!important$/, "");
  return { property, value, important };
}

interface Sheet {
  rules: Rule[];
  /** Initial values of registered custom properties (`@property`). */
  initial: Record<string, string>;
}

function flatten(source: string, layerOf: (name: string | null) => number): Sheet {
  const rules: Rule[] = [];
  const initial: Record<string, string> = {};
  let order = 0;
  const walk = (nodes: Node[], selectors: string[] | null, layer: string | null, media: string[]) => {
    const own: Declaration[] = [];
    const rule: Rule | null = selectors ? { selectors, declarations: own, layer: layerOf(layer), media, order: order++ } : null;
    if (rule) rules.push(rule);
    for (const node of nodes) {
      if (node.kind === "text") {
        if (selectors && !node.text.startsWith("@")) {
          const parsed = declaration(node.text);
          if (parsed) own.push(parsed);
        }
        continue;
      }
      const { prelude, children } = node;
      if (prelude.startsWith("@layer")) walk(children, selectors, prelude.slice(6).trim(), media);
      else if (prelude.startsWith("@media")) walk(children, selectors, layer, [...media, prelude.slice(6).trim()]);
      else if (prelude.startsWith("@supports")) walk(children, selectors, layer, [...media, `supports ${prelude.slice(9).trim()}`]);
      else if (prelude.startsWith("@property")) {
        const name = prelude.slice(9).trim();
        for (const child of children) {
          const parsed = child.kind === "text" ? declaration(child.text) : null;
          if (parsed?.property === "initial-value") initial[name] = parsed.value;
        }
      } else if (prelude.startsWith("@")) continue;
      else walk(children, nest(selectors, splitList(prelude)), layer, media);
    }
  };
  walk(parse(source), null, null, []);
  return { rules, initial };
}

export interface Environment {
  reducedMotion?: boolean;
}

function mediaMatches(condition: string, environment: Environment): boolean {
  const query = condition.replace(/\s+/g, " ").trim();
  if (query === "(hover: hover)") return true;
  if (query === "(prefers-reduced-motion: reduce)") return environment.reducedMotion === true;
  if (query === "(prefers-reduced-motion: no-preference)") return environment.reducedMotion !== true;
  // print, forced colours, @supports fallbacks for other engines, colour scheme (the app
  // switches themes with data-theme, not media queries): not the state under test.
  return false;
}

const PSEUDO_ELEMENT = /::?(before|after|placeholder|backdrop|file-selector-button|selection|marker|-webkit-[a-z-]+|-moz-[a-z-]+)/;

function stripFunctional(selector: string, name: string): { rest: string; inner: string[] } {
  const inner: string[] = [];
  let rest = "";
  let index = 0;
  const token = `:${name}(`;
  while (index < selector.length) {
    const start = selector.indexOf(token, index);
    if (start < 0) {
      rest += selector.slice(index);
      break;
    }
    rest += selector.slice(index, start);
    let depth = 1;
    let end = start + token.length;
    while (end < selector.length && depth > 0) {
      if (selector[end] === "(") depth += 1;
      if (selector[end] === ")") depth -= 1;
      end += 1;
    }
    inner.push(selector.slice(start + token.length, end - 1));
    index = end;
  }
  return { rest, inner };
}

/** Specificity as one comparable number (ids, classes, types). */
export function specificity(selector: string): number {
  let rest = stripFunctional(selector, "where").rest;
  let extra = 0;
  for (const name of ["is", "not", "has"]) {
    const stripped = stripFunctional(rest, name);
    rest = stripped.rest;
    for (const inner of stripped.inner) extra += Math.max(0, ...splitList(inner).map(specificity));
  }
  const unescaped = rest.replace(/\\./g, "x");
  const ids = (unescaped.match(/#[\w-]+/g) ?? []).length;
  const classes = (unescaped.match(/\.[\w-]+|\[[^\]]*\]|:(?!:)[\w-]+/g) ?? []).length;
  const types = (unescaped.replace(/\[[^\]]*\]/g, "").match(/(^|[\s>+~(])[a-z][\w-]*/gi) ?? []).length;
  return ids * 10_000 + classes * 100 + types + extra;
}

function selectorMatches(element: Element, selector: string): boolean {
  if (PSEUDO_ELEMENT.test(selector)) return false;
  // Not the state under test; jsdom would never match them anyway.
  const state = selector.replace(/:(hover|active)\b/g, ":not(*)");
  try {
    return element.matches(state);
  } catch {
    return false;
  }
}

export interface Stylesheet {
  /** The winning value of `property` on `element` in its current state, `var()` left as is. */
  value(element: Element, property: string, environment?: Environment): string | undefined;
  /** Same, with `var()` resolved against the element's own custom properties and `@property`. */
  resolved(element: Element, property: string, environment?: Environment): string | undefined;
}

const SHORTHANDS: Record<string, (value: string) => Record<string, string>> = {
  outline: (value) => {
    const result: Record<string, string> = { "outline-width": "medium", "outline-style": "none", "outline-color": "currentcolor" };
    for (const part of value.split(/\s+(?![^(]*\))/)) {
      if (/^(none|hidden|solid|dashed|dotted|double|groove|ridge|inset|outset|auto)$/.test(part)) result["outline-style"] = part;
      else if (/^(-?[\d.]+(px|rem|em)?|thin|medium|thick|calc\(.*\))$/.test(part)) result["outline-width"] = part;
      else result["outline-color"] = part;
    }
    return result;
  },
  transition: (value) => {
    const durations = value.match(/-?[\d.]+m?s\b|var\([^)]*duration[^)]*\)/g) ?? [];
    return { "transition-duration": durations[0] ?? "0s", "transition-property": value.split(/\s+/)[0] ?? "all" };
  },
};

function build(sheet: Sheet): Stylesheet {
  const candidatesFor = (property: string) =>
    sheet.rules.flatMap((rule) =>
      rule.declarations.flatMap((declaration) => {
        if (declaration.property === property) return [{ rule, value: declaration.value, important: declaration.important }];
        const expand = SHORTHANDS[declaration.property];
        const expanded = expand?.(declaration.value)[property];
        return expanded === undefined ? [] : [{ rule, value: expanded, important: declaration.important }];
      }),
    );
  const index = new Map<string, ReturnType<typeof candidatesFor>>();

  const value = (element: Element, property: string, environment: Environment = {}): string | undefined => {
    const inline = element instanceof HTMLElement ? element.style.getPropertyValue(property) : "";
    if (inline) return inline;
    let candidates = index.get(property);
    if (!candidates) {
      candidates = candidatesFor(property);
      index.set(property, candidates);
    }
    let best: { rank: number[]; value: string } | undefined;
    for (const candidate of candidates) {
      if (!candidate.rule.media.every((condition) => mediaMatches(condition, environment))) continue;
      const matching = candidate.rule.selectors.filter((selector) => selectorMatches(element, selector));
      if (matching.length === 0) continue;
      const layer = candidate.important ? UNLAYERED * 2 - candidate.rule.layer : candidate.rule.layer;
      const rank = [candidate.important ? 1 : 0, layer, Math.max(...matching.map(specificity)), candidate.rule.order];
      if (!best || compareRank(rank, best.rank) >= 0) best = { rank, value: candidate.value };
    }
    return best?.value;
  };

  const resolved = (element: Element, property: string, environment: Environment = {}): string | undefined => {
    const raw = value(element, property, environment);
    if (raw === undefined) return undefined;
    return resolveVars(raw, (name) => {
      // Design tokens stay named: the check is "uses the focus-ring token", not a hex value.
      if (name.startsWith("--ui-")) return undefined;
      return value(element, name, environment) ?? sheet.initial[name];
    });
  };

  return { value, resolved };
}

function compareRank(a: number[], b: number[]): number {
  for (let i = 0; i < a.length; i += 1) {
    const difference = (a[i] ?? 0) - (b[i] ?? 0);
    if (difference !== 0) return difference;
  }
  return 0;
}

function resolveVars(text: string, lookup: (name: string) => string | undefined, depth = 0): string {
  if (depth > 10) return text;
  const start = text.indexOf("var(");
  if (start < 0) return text;
  let depthCount = 1;
  let end = start + 4;
  while (end < text.length && depthCount > 0) {
    if (text[end] === "(") depthCount += 1;
    if (text[end] === ")") depthCount -= 1;
    end += 1;
  }
  const inner = text.slice(start + 4, end - 1);
  const comma = inner.indexOf(",");
  const name = (comma < 0 ? inner : inner.slice(0, comma)).trim();
  const fallback = comma < 0 ? undefined : inner.slice(comma + 1).trim();
  const found = lookup(name);
  const replacement = found ?? (fallback !== undefined ? resolveVars(fallback, lookup, depth + 1) : `var(${name})`);
  return resolveVars(text.slice(0, start) + replacement + text.slice(end), lookup, depth + 1);
}

function classCandidates(root: ParentNode): string[] {
  const classes = new Set<string>();
  for (const element of root.querySelectorAll("[class]")) {
    for (const name of (element.getAttribute("class") ?? "").split(/\s+/)) if (name) classes.add(name);
  }
  return [...classes];
}

let compiler: ReturnType<typeof compile> | undefined;

function tailwind() {
  compiler ??= compile(appCss, {
    base: "/src/styles",
    loadStylesheet: (id: string, base: string) => {
      if (id === "tailwindcss") return Promise.resolve({ path: "tailwindcss/index.css", base, content: tailwindIndexCss });
      if (id === "./tokens.css") return Promise.resolve({ path: "/src/styles/tokens.css", base, content: tokensCss });
      return Promise.reject(new Error(`app.css imports ${id}, which tests/support/styles.ts does not know`));
    },
  });
  return compiler;
}

/** The app's stylesheet for the classes on the page right now (Tailwind plus composer.css). */
export async function appStylesheet(root: ParentNode = document): Promise<Stylesheet> {
  const css = (await tailwind()).build(classCandidates(root));
  const tailwindSheet = flatten(css, (name) => (name === null ? UNLAYERED : (LAYERS[name] ?? UNLAYERED)));
  // composer.css is a plain stylesheet imported by the editor: unlayered, after Tailwind.
  const composerSheet = flatten(composerCss, () => UNLAYERED);
  const offset = tailwindSheet.rules.length;
  return build({
    rules: [...tailwindSheet.rules, ...composerSheet.rules.map((rule) => ({ ...rule, order: rule.order + offset }))],
    initial: tailwindSheet.initial,
  });
}

/** The compiled CSS for some class names, for checks on the generated rules themselves. */
export async function compiledCss(classes: string[]): Promise<string> {
  return (await tailwind()).build(classes);
}
