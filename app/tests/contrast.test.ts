import { describe, expect, it } from "vitest";
import tokensCss from "../src/styles/tokens.css?raw";
import { contrast, deltaE, luminance } from "./support/color";
import { ACCENT_THEMES, DEFAULT_ACCENT } from "../src/features/preferences/accent";

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

// Stage 16: the accent themes. Each changes only the accent family, in light and in dark.
const ACCENT_FAMILY = ["accent", "accent-soft", "selected", "focus-ring", "text-on-accent"];
const accentLight = (accent: string) => colourTokens(block(new RegExp(`(?:^|\\n)\\[data-accent="${accent}"\\]\\s*\\{([^}]*)\\}`)));
const accentDark = (accent: string) =>
  colourTokens(block(new RegExp(`:root\\[data-theme="dark"\\] \\[data-accent="${accent}"\\]\\s*\\{([^}]*)\\}`)));
const accentThemes = Object.fromEntries(
  ACCENT_THEMES.flatMap((accent) => [
    [`light ${accent}`, { ...light, ...accentLight(accent) }],
    [`dark ${accent}`, { ...dark, ...accentDark(accent) }],
  ]),
);
const themes: Record<string, Tokens> = { light, dark, ...accentThemes };

// Decision 0031: `surface-list` is the list column's own tinted surface.
const SURFACES = ["canvas", "surface", "surface-list", "surface-raised", "rail", "priority-tint", "selected", "hover"];
const ACCOUNTS = Array.from({ length: 8 }, (_, i) => `account-${i + 1}`);
/** Decision 0031: avatar tints, initials (`-text`) on their fill (`-bg`). */
const AVATAR_TINTS = Array.from({ length: 8 }, (_, i): [string, string] => [`avatar-${i + 1}-text`, `avatar-${i + 1}-bg`]);

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
  // Stage 4. Notices (raised surface): the Undo button, the check and error symbols. The
  // selection bar on the soft accent: count, quiet buttons, toolbar symbols. The count on the
  // activity badge (danger fill, or secondary on the rail). Failure texts in the activity view.
  ...["accent", "success", "danger"].map((fg): [string, string] => [fg, "surface-raised"]),
  ...["text-primary", "text-secondary", "accent"].map((fg): [string, string] => [fg, "accent-soft"]),
  ["text-on-accent", "danger"],
  ["text-secondary", "rail"],
  ["danger", "canvas"],
  // Decision 0031 (Feinschliff). Initials on every avatar tint; the conversation size on its pill;
  // hovered entries of the sidebar on the rail's hover tone; "Show all" and the way back in the
  // accent on the priority card.
  ...AVATAR_TINTS,
  ["text-secondary", "badge"],
  ["text-primary", "badge"],
  ["text-primary", "rail-hover"],
  ["text-secondary", "rail-hover"],
  ["text-tertiary", "rail-hover"],
  ["accent", "priority-tint"],
  ["accent", "surface-list"],
  // Stage 16, find in the open message: highlighted text (it takes text-primary) and the current
  // match with its own text colour.
  ["text-primary", "find-match"],
  ["find-current-text", "find-current"],
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
  // Stage 4, actions. Symbols for pending (tertiary), failed (danger) and conflicting (warning)
  // changes in rows, plain, selected and hovered. The selection box: its border (secondary) and
  // the checked fill (accent). The ring of the activity badge on the rail. Drop target outline on
  // the soft accent. The write switch: track off (border-strong) and on (accent) on the surface,
  // the knob on the accent and on the surface.
  ...["text-tertiary", "danger", "warning", "text-secondary", "accent"].flatMap((fg) =>
    ["surface", "selected", "hover"].map((bg): [string, string] => [fg, bg]),
  ),
  ["text-secondary", "surface-raised"],
  ["border-strong", "rail"],
  // Decision 0031. Account rings, unread dot and flag on the tinted list column; the priority
  // symbol on the card; icons and the active bar on the rail's hover tone; the tile icons of the
  // bundles on their tint.
  ...ACCOUNTS.map((account): [string, string] => [account, "surface-list"]),
  ...["accent", "warning", "danger", "text-tertiary"].map((fg): [string, string] => [fg, "surface-list"]),
  ["warning", "priority-tint"],
  ["text-secondary", "rail-hover"],
  ["accent", "rail-hover"],
  ["focus-ring", "rail-hover"],
  ["avatar-1-text", "avatar-1-bg"],
  ["avatar-5-text", "avatar-5-bg"],
  ["accent", "accent-soft"],
  ["accent", "surface"],
  ["surface-raised", "accent"],
  ["text-secondary", "surface"],
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
      // Decision 0031
      "rail-hover",
      "badge",
      "border-subtle",
      "priority-border",
      ...AVATAR_TINTS.flat(),
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
  for (const name of [...SURFACES, "rail-hover", "badge", ...AVATAR_TINTS.flat()]) expect(dark[name], name).not.toBe(light[name]);
});

it("orders the surfaces as decision 0031 says: rail darkest, list column tinted, reading pane brightest", () => {
  // Light: lower luminance is darker. Dark: the rail is the darkest step too.
  expect(luminance(light["rail"] ?? "")).toBeLessThan(luminance(light["surface-list"] ?? ""));
  expect(luminance(light["surface-list"] ?? "")).toBeLessThan(luminance(light["surface"] ?? ""));
  expect(luminance(dark["rail"] ?? "")).toBeLessThan(luminance(dark["surface-list"] ?? ""));
  expect(luminance(dark["surface-list"] ?? "")).toBeLessThan(luminance(dark["surface"] ?? ""));
});

it("keeps the eight avatar tints apart from each other in both themes", () => {
  for (const tokens of [light, dark]) {
    for (const [i, [, a]] of AVATAR_TINTS.entries()) {
      for (const [, b] of AVATAR_TINTS.slice(i + 1)) {
        expect(deltaE(tokens[a] ?? "", tokens[b] ?? ""), `${a} vs ${b}`).toBeGreaterThan(5);
      }
    }
  }
});

describe("accent themes (stage 16)", () => {
  it("define the accent family and nothing else, in light and dark", () => {
    for (const accent of ACCENT_THEMES) {
      for (const tokens of [accentLight(accent), accentDark(accent)]) {
        expect(Object.keys(tokens).sort(), accent).toEqual([...ACCENT_FAMILY].sort());
      }
    }
  });

  it("the default's block repeats the default values (the picker's swatch shows it)", () => {
    for (const name of ACCENT_FAMILY) {
      expect(accentLight(DEFAULT_ACCENT)[name], name).toBe(light[name]);
      expect(accentDark(DEFAULT_ACCENT)[name], name).toBe(dark[name]);
    }
  });

  it("are told apart from each other in both modes", () => {
    for (const mode of ["light", "dark"]) {
      for (const [i, a] of ACCENT_THEMES.entries()) {
        for (const b of ACCENT_THEMES.slice(i + 1)) {
          const ta = themes[`${mode} ${a}`]?.["accent"] ?? "";
          const tb = themes[`${mode} ${b}`]?.["accent"] ?? "";
          expect(deltaE(ta, tb), `${mode}: ${a} vs ${b}`).toBeGreaterThan(15);
        }
      }
    }
  });
});

it("the contrast check itself is calibrated (black on white is 21:1)", () => {
  expect(contrast("#000000", "#ffffff")).toBeCloseTo(21, 5);
  expect(contrast("#767676", "#ffffff")).toBeGreaterThanOrEqual(4.5);
});
