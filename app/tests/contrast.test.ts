import { describe, expect, it } from "vitest";
import tokensCss from "../src/styles/tokens.css?raw";
import { contrast, deltaE, luminance, parseHex } from "./support/color";
import { ACCENT_THEMES } from "../src/features/preferences/accent";

/**
 * The accent of the root values of tokens.css. Since decision 0053 the root holds the default
 * theme's (blue's) values, so the first frame has them before the core answers; the chosen theme
 * is set on the root like every other one, and blue's block repeats the root values.
 */
const ROOT_ACCENT = "blue";

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
const ACCENT_FAMILY = ["accent", "accent-fill", "accent-soft", "selected", "focus-ring", "text-on-accent"];
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

// Decision 0031: `surface-list` is the list column's own surface. Stage 23 (decision 0083): the
// sidebar's selected entry, the toolbar, the date bands, the contact panel and buttons.
const SURFACES = [
  "canvas",
  "surface",
  "surface-list",
  "surface-raised",
  "rail",
  "priority-tint",
  "selected",
  "hover",
  "rail-selected",
  "toolbar",
  "band",
  "panel",
  "button",
];
const FILE_TYPES = ["pdf", "doc", "sheet", "image", "archive"];
const ACCOUNTS = Array.from({ length: 8 }, (_, i) => `account-${i + 1}`);

const TEXT_PAIRS: [string, string][] = [
  ...["text-primary", "text-secondary", "text-tertiary"].flatMap((fg) => SURFACES.map((bg): [string, string] => [fg, bg])),
  ["text-primary", "accent-soft"],
  ["text-secondary", "accent-soft"],
  // Design pass 23.11.2026: the label of primary buttons and other fills sits on `accent-fill`.
  ["text-on-accent", "accent-fill"],
  ...["canvas", "surface", "surface-raised", "rail", "selected", "accent-soft"].map((bg): [string, string] => ["accent", bg]),
  ...["danger", "success", "warning"].flatMap((fg) =>
    ["canvas", "surface", "surface-raised"].map((bg): [string, string] => [fg, bg]),
  ),
  // Stage 2: label of the danger button ("Remove"), quiet buttons on hover.
  ["text-on-danger", "danger"],
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
  ["text-on-danger", "danger"],
  ["text-secondary", "rail"],
  ["danger", "canvas"],
  // Decision 0031 (Feinschliff). The conversation size on its pill; hovered entries of the
  // sidebar on the rail's hover tone; "Show all" and the way back in the accent on the priority
  // card. (The avatar tints went with the avatars, stage 22, decision 0072.)
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
  // Design pass 23.11.2026: state pills (outbox, activity) on the badge fill.
  ...["text-secondary", "warning", "danger", "success"].map((fg): [string, string] => [fg, "badge"]),
  // Labels of segmented controls on their track (the checked one sits on a raised thumb).
  ["text-primary", "track"],
  ["text-secondary", "track"],
  ["text-primary", "surface-raised"],
  // Stage 23 (decision 0083). Counts and labels on the chips (`badge`); accent text on the
  // toolbar, bands, panel, buttons and the sidebar's hover ("Cc", links, quiet buttons).
  // Attachment cards: name and size on the type's tint, the badge's label on its fill.
  ["text-tertiary", "badge"],
  ...["toolbar", "band", "panel", "button", "rail-hover"].map((bg): [string, string] => ["accent", bg]),
  ...FILE_TYPES.flatMap((type) =>
    ["text-primary", "text-secondary", "text-tertiary"].map((fg): [string, string] => [fg, `file-${type}-tint`]),
  ),
  ...FILE_TYPES.map((type): [string, string] => ["text-on-file", `file-${type}`]),
];

const CONTROL_PAIRS: [string, string][] = [
  ...[...SURFACES, "accent-soft"].map((bg): [string, string] => ["focus-ring", bg]),
  ...["canvas", "surface", "surface-raised"].map((bg): [string, string] => ["border-strong", bg]),
  // Checked segment on its track, active-item bars on rail and selected tab.
  ["accent", "canvas"],
  ["accent", "rail"],
  ["accent", "selected"],
  ["danger", "surface"],
  // Stage 2. Account colours in list rows (plain, selected, hovered) and in the rail; since stage 22
  // (decision 0072) as dots, before the avatars' rings.
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
  // Decision 0031. Account dots, unread dot and flag on the tinted list column; the priority
  // symbol on the card; icons and the active bar on the rail's hover tone.
  ...ACCOUNTS.map((account): [string, string] => [account, "surface-list"]),
  ...["accent", "warning", "danger", "text-tertiary"].map((fg): [string, string] => [fg, "surface-list"]),
  ["warning", "priority-tint"],
  ["text-secondary", "rail-hover"],
  ["accent", "rail-hover"],
  ["focus-ring", "rail-hover"],
  ["accent", "accent-soft"],
  ["accent", "surface"],
  ["surface-raised", "accent"],
  ["text-secondary", "surface"],
  // Design pass 23.11.2026: the accent ring of the checked segment on the track; the switch's
  // off knob on the track.
  ["accent", "track"],
  ["text-secondary", "track"],
  // Stage 23: the icon of the sidebar's selected entry in the accent; account colours as icons
  // and squares in the sidebar (on its hover and selected fills too) and as the stripe of rows on
  // the bands; field borders on the toolbar and panel.
  ["accent", "rail-selected"],
  ...ACCOUNTS.flatMap((account) => ["rail-hover", "rail-selected", "band", "toolbar"].map((bg): [string, string] => [account, bg])),
  ...["toolbar", "panel", "button", "band"].map((bg): [string, string] => ["border-strong", bg]),
  ...["toolbar", "button", "rail-selected", "band", "panel"].map((bg): [string, string] => ["focus-ring", bg]),
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
      // Design pass 23.11.2026
      "accent-fill",
      "text-on-danger",
      "track",
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
      // Stage 23 (decision 0083)
      ...FILE_TYPES.flatMap((type) => [`file-${type}`, `file-${type}-tint`]),
      "text-on-file",
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
  for (const name of [...SURFACES, "rail-hover", "badge"]) expect(dark[name], name).not.toBe(light[name]);
});

it("orders the surfaces as decision 0083 says: one content surface, the chrome a step apart, bands between", () => {
  // Stage 23 replaces the three steps of decision 0031 (rail darkest, list tinted, reading pane
  // brightest) with the approved draft: list and reading pane are one surface; sidebar and
  // toolbar are a step darker in light and a step lighter in dark; the content is the extreme.
  for (const tokens of [light, dark]) {
    expect(tokens["surface-list"]).toBe(tokens["surface"]);
    expect(tokens["toolbar"], "toolbar").not.toBe(tokens["surface"]);
  }
  const l = (tokens: Tokens, name: string) => luminance(tokens[name] ?? "");
  for (const name of ["rail", "toolbar", "band", "panel", "rail-selected"]) {
    expect(l(light, name), `light ${name}`).toBeLessThan(l(light, "surface"));
    expect(l(dark, name), `dark ${name}`).toBeGreaterThan(l(dark, "surface"));
  }
  // The selected entry of the sidebar stands out from the sidebar and its hover.
  expect(l(light, "rail-selected")).toBeLessThan(l(light, "rail-hover"));
  expect(l(light, "rail-hover")).toBeLessThan(l(light, "rail"));
  expect(l(dark, "rail-selected")).toBeGreaterThan(l(dark, "rail-hover"));
  expect(l(dark, "rail-hover")).toBeGreaterThan(l(dark, "rail"));
});

it("uses the approved draft's colours (stage 23, decision 0083)", () => {
  const draft = {
    light: { surface: "#ffffff", rail: "#f2f4f7", band: "#f6f7f9", "border-subtle": "#e5e8ec", "text-primary": "#131a24", "text-secondary": "#455061", "text-tertiary": "#5b6676", "accent-fill": "#1463d8", "rail-selected": "#e2e8f1", badge: "#eef1f5", toolbar: "#f7f8fa", button: "#ffffff", border: "#dde1e7", panel: "#f8f9fb" },
    dark: { surface: "#14181e", rail: "#1a1f27", toolbar: "#1a1f27", band: "#1b2129", "border-subtle": "#29303b", "text-primary": "#e8edf3", "text-secondary": "#bcc5d0", "text-tertiary": "#98a3b0", accent: "#6aa8ff", "accent-fill": "#2563eb", "rail-selected": "#2a3341", badge: "#252c37", button: "#232a34", border: "#333b48", "surface-raised": "#1b2129", panel: "#171c23" },
  };
  expect(Object.fromEntries(Object.keys(draft.light).map((k) => [k, light[k]]))).toEqual(draft.light);
  expect(Object.fromEntries(Object.keys(draft.dark).map((k) => [k, dark[k]]))).toEqual(draft.dark);
});

it("keeps the neutrals cool in both themes, never warm (decision 0053)", () => {
  // Surfaces, lines and text greys lean towards blue: blue never below red.
  const NEUTRALS = [
    "canvas",
    "surface",
    "surface-list",
    "rail",
    "rail-hover",
    "hover",
    "badge",
    "track",
    "priority-tint",
    "rail-selected",
    "toolbar",
    "band",
    "panel",
    "button",
    "priority-border",
    "border",
    "border-subtle",
    "border-strong",
    "text-primary",
    "text-secondary",
    "text-tertiary",
  ];
  for (const [mode, tokens] of [["light", light], ["dark", dark]] as const) {
    for (const name of NEUTRALS) {
      const [red, , blue] = parseHex(tokens[name] ?? "");
      expect(blue, `${mode} ${name}`).toBeGreaterThanOrEqual(red);
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

  it("the root's accent block repeats the root values (the picker's swatch shows it)", () => {
    for (const name of ACCENT_FAMILY) {
      expect(accentLight(ROOT_ACCENT)[name], name).toBe(light[name]);
      expect(accentDark(ROOT_ACCENT)[name], name).toBe(dark[name]);
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

/*
 * Stage 22 (decision 0079): with Mica on Windows 11 the frame, the icon rail and the docked folder
 * column are transparent (app.css), so whatever the UI draws on `rail` or `canvas` there lies on
 * Mica instead. Mica is Windows' own material: the wallpaper, blurred, its lightness replaced by
 * a fixed tone and tinted with it (Windows App SDK's MicaController: light #f3f3f3 at 50 % tint,
 * dark #202020 at 80 %, luminosity at 100 %), so only its hue follows the wallpaper: at most half
 * (light) or a fifth (dark) of it is the wallpaper's colour at the tone's own lightness. Every
 * pair on `rail` and `canvas` is checked against the darkest light and the lightest dark Mica
 * that leaves room for, with margin: #e6e6e6 and #282828. (Cards drawn on `canvas` inside the
 * opaque content never lie on Mica; checking them too costs nothing.) What Mica really looks like
 * over a given wallpaper is for the check on Windows (docs/testing/windows-checklist.md).
 */
const MICA = { light: "#e6e6e6", dark: "#282828" } as const;
// Stage 23: the sidebar's hover and selected fills stay opaque on Mica, so they are not here.
const ON_CHROME = new Set(["rail", "canvas"]);

describe.each(Object.entries(themes))("%s theme on Mica", (name, tokens) => {
  const mica = name.startsWith("dark") ? MICA.dark : MICA.light;
  function colour(token: string): string {
    const value = tokens[token];
    if (!value) throw new Error(`token --ui-${token} is missing`);
    return value;
  }

  it.each(TEXT_PAIRS.filter(([, bg]) => ON_CHROME.has(bg)))("text %s on Mica (instead of %s) reaches 4.5:1", (fg) => {
    expect(contrast(colour(fg), mica)).toBeGreaterThanOrEqual(4.5);
  });

  it.each(CONTROL_PAIRS.filter(([, bg]) => ON_CHROME.has(bg)))("control %s on Mica (instead of %s) reaches 3:1", (fg) => {
    expect(contrast(colour(fg), mica)).toBeGreaterThanOrEqual(3);
  });
});
