import { describe, expect, it } from "vitest";
import tokensCss from "../src/styles/tokens.css?raw";
import { contrast, deltaE } from "./support/color";

// Contrast rules from docs/SPEC.md (Barrierefreiheit): text >= 4.5:1, controls and other
// meaningful graphics >= 3:1, in both themes. The pairs below are the ones the UI uses.

type Tokens = Record<string, string>;

function colourTokens(block: string): Tokens {
  const tokens: Tokens = {};
  for (const match of block.matchAll(/--ui-([a-z0-9-]+):\s*(#[0-9a-f]{6})\s*;/gi)) {
    const [, name, value] = match;
    if (name && value) tokens[name] = value.toLowerCase();
  }
  return tokens;
}

function block(selector: RegExp): string {
  const match = selector.exec(tokensCss);
  if (!match?.[1]) throw new Error(`block ${String(selector)} not found in tokens.css`);
  return match[1];
}

const light = colourTokens(block(/:root\s*\{([^}]*)\}/));
const dark = { ...light, ...colourTokens(block(/:root\[data-theme="dark"\]\s*\{([^}]*)\}/)) };
const themes = { light, dark };

const SURFACES = ["canvas", "surface", "surface-raised", "rail", "priority-tint", "selected", "hover"];
const ACCOUNTS = Array.from({ length: 8 }, (_, i) => `account-${i + 1}`);

const TEXT_PAIRS: [string, string][] = [
  ...["text-primary", "text-secondary", "text-tertiary"].flatMap((fg) => SURFACES.map((bg): [string, string] => [fg, bg])),
  ["text-primary", "accent-soft"],
  ["text-secondary", "accent-soft"],
  ["text-on-accent", "accent"],
  ...["canvas", "surface", "surface-raised", "rail", "selected", "accent-soft"].map((bg): [string, string] => ["accent", bg]),
  ...["danger", "success", "warning"].flatMap((fg) =>
    ["canvas", "surface", "surface-raised"].map((bg): [string, string] => [fg, bg]),
  ),
  // Stage 2: label of the danger button ("Remove"), quiet buttons on hover.
  ["text-on-accent", "danger"],
  ["accent", "hover"],
  // Stage 3, reading pane: link text in plain-text mail on the pane, the emphasised host of a
  // link target, the warning texts of attachments, the saved note, the link card's surface.
  ["accent", "surface"],
  ["text-primary", "accent-soft"],
  ["warning", "canvas"],
  ["success", "canvas"],
  ...["text-primary", "text-secondary"].map((fg): [string, string] => [fg, "surface-raised"]),
];

const CONTROL_PAIRS: [string, string][] = [
  ...[...SURFACES, "accent-soft"].map((bg): [string, string] => ["focus-ring", bg]),
  ...["canvas", "surface", "surface-raised"].map((bg): [string, string] => ["border-strong", bg]),
  // Checked segment on its track, active-item bars on rail and selected tab.
  ["accent", "canvas"],
  ["accent", "rail"],
  ["accent", "selected"],
  ["danger", "surface"],
  // Stage 2. Account rings on avatars in list rows (plain, selected, hovered) and in the rail.
  ...ACCOUNTS.flatMap((account) =>
    ["canvas", "surface", "surface-raised", "rail", "selected", "hover"].map((bg): [string, string] => [account, bg]),
  ),
  // Unread dot and flag in list rows, error badge on the rail's account entry.
  ...["accent", "warning"].flatMap((fg) => ["surface", "selected", "hover"].map((bg): [string, string] => [fg, bg])),
  ["danger", "rail"],
  ["text-secondary", "rail"],
  // Stage 3, reading pane. The pressed HTML/Text switch on its track; the border of the white
  // mail canvas and of attachments that can run code; the warning symbol on cards.
  ["accent", "canvas"],
  ["border-strong", "surface"],
  ["warning", "canvas"],
  ["warning", "surface-raised"],
];

describe.each(Object.entries(themes))("%s theme", (_name, tokens) => {
  function colour(name: string): string {
    const value = tokens[name];
    if (!value) throw new Error(`token --ui-${name} is missing`);
    return value;
  }

  it("defines every colour token from DESIGN.md", () => {
    const expected = [
      ...SURFACES,
      "border",
      "border-strong",
      "focus-ring",
      "text-primary",
      "text-secondary",
      "text-tertiary",
      "text-on-accent",
      "accent",
      "accent-soft",
      "danger",
      "success",
      "warning",
      ...ACCOUNTS,
      "mail-canvas",
    ];
    for (const name of expected) expect(tokens, name).toHaveProperty(name);
  });

  it.each(TEXT_PAIRS)("text %s on %s reaches 4.5:1", (fg, bg) => {
    expect(contrast(colour(fg), colour(bg))).toBeGreaterThanOrEqual(4.5);
  });

  it.each(CONTROL_PAIRS)("control %s on %s reaches 3:1", (fg, bg) => {
    expect(contrast(colour(fg), colour(bg))).toBeGreaterThanOrEqual(3);
  });

  it("keeps the eight account colours apart from each other", () => {
    for (const [i, a] of ACCOUNTS.entries()) {
      for (const b of ACCOUNTS.slice(i + 1)) {
        expect(deltaE(colour(a), colour(b)), `${a} vs ${b}`).toBeGreaterThan(15);
      }
    }
  });
});

it("HTML mail keeps its own white canvas in the dark theme (DESIGN.md: never recoloured)", () => {
  expect(light["mail-canvas"]).toBe("#ffffff");
  expect(dark["mail-canvas"]).toBe(light["mail-canvas"]);
  expect(colourTokens(block(/:root\[data-theme="dark"\]\s*\{([^}]*)\}/))).not.toHaveProperty("mail-canvas");
});

it("dark theme has its own surface steps instead of reusing light ones", () => {
  for (const name of SURFACES) expect(dark[name], name).not.toBe(light[name]);
});

it("the contrast check itself is calibrated (black on white is 21:1)", () => {
  expect(contrast("#000000", "#ffffff")).toBeCloseTo(21, 5);
  expect(contrast("#767676", "#ffffff")).toBeGreaterThanOrEqual(4.5);
});
