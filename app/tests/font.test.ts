import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import tauriConf from "../src-tauri/tauri.conf.json?raw";
import composerCss from "../src/features/composer/composer.css?raw";
import appCss from "../src/styles/app.css?raw";
import tokensCss from "../src/styles/tokens.css?raw";

// Stage 23, item 2 (decision 0083): the UI font is Inter, shipped with the app and never loaded
// from the network; mail content keeps the system font.

const here = dirname(fileURLToPath(import.meta.url));
const FONT_DIR = join(here, "..", "src", "assets", "fonts");

/**
 * The unmodified `web/InterVariable.woff2` of Inter 4.1
 * (https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip, whose SHA-256 is
 * 9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e).
 */
const INTER_SHA256 = "693b77d4f32ee9b8bfc995589b5fad5e99adf2832738661f5402f9978429a8e3";

function token(name: string): string {
  const match = new RegExp(`--ui-${name}:\\s*([^;]+);`).exec(tokensCss);
  if (!match?.[1]) throw new Error(`--ui-${name} not found in tokens.css`);
  return match[1].replace(/\s+/g, " ").trim();
}

const families = (list: string) => list.split(",").map((f) => f.trim().replace(/^"|"$/g, ""));

describe("the UI font", () => {
  it("is the release's variable Inter, with its licence next to it", () => {
    const font = readFileSync(join(FONT_DIR, "InterVariable.woff2"));
    expect(font.subarray(0, 4).toString("latin1")).toBe("wOF2");
    expect(createHash("sha256").update(font).digest("hex")).toBe(INTER_SHA256);
    const licence = readFileSync(join(FONT_DIR, "LICENSE.txt"), "utf8");
    expect(licence).toMatch(/The Inter Project Authors/);
    expect(licence).toMatch(/SIL OPEN FONT LICENSE Version 1\.1/);
  });

  it("is declared once, from the app's own file, for every weight", () => {
    const faces = [...appCss.matchAll(/@font-face\s*\{([^}]*)\}/g)].map((m) => m[1] ?? "");
    expect(faces).toHaveLength(1);
    const face = faces[0] ?? "";
    expect(face).toMatch(/font-family:\s*"InterVariable";/);
    expect(face).toMatch(/font-weight:\s*100 900;/);
    expect(face).toMatch(/src:\s*url\("\.\.\/assets\/fonts\/InterVariable\.woff2"\) format\("woff2"\);/);
  });

  it("is never loaded from the network, and the CSP allows only the app's own fonts", () => {
    for (const css of [appCss, tokensCss, composerCss]) {
      expect(css).not.toMatch(/url\(\s*["']?(?:https?:)?\/\//i);
      expect(css).not.toMatch(/@import\s+(?:url\()?["']?https?:/i);
    }
    const security = (JSON.parse(tauriConf) as { app: { security: { csp: string; devCsp: string } } }).app.security;
    for (const csp of [security.csp, security.devCsp]) {
      const directives = new Map(
        csp.split(";").map((d) => {
          const [name = "", ...values] = d.trim().split(/\s+/);
          return [name, values] as const;
        }),
      );
      // `font-src` falls back to `default-src`.
      const fontSrc = directives.get("font-src") ?? directives.get("default-src");
      expect(fontSrc).toEqual(["'self'"]);
    }
  });

  it("comes first in the UI's font list, with the system font behind it; mail content keeps the system font", () => {
    const ui = families(token("font-family"));
    const mail = families(token("font-family-mail"));
    expect(ui[0]).toBe("InterVariable");
    expect(ui.slice(1)).toEqual(mail);
    expect(mail).toEqual(expect.arrayContaining(["-apple-system", "Segoe UI Variable Text", "system-ui", "sans-serif"]));
    expect(mail.join(",")).not.toMatch(/Inter/);
    // The composer's writing area and the Tailwind `font-mail` utility use the mail font.
    expect(composerCss).toMatch(/\.composer-prose \{\s*font-family: var\(--ui-font-family-mail\);/);
    expect(appCss).toMatch(/--font-mail: var\(--ui-font-family-mail\);/);
    // Character variants of the draft for the UI only: not for mail or code.
    expect(token("font-features")).toBe('"cv11", "ss01"');
    expect(appCss).toMatch(/--font-mail--font-feature-settings: normal;/);
    expect(appCss).toMatch(/--font-mono--font-feature-settings: normal;/);
  });
});
