// Proves that the embedded WebDriver is only in test builds.
//
// - Positive control: the e2e build (`pnpm build:e2e`) must contain the marker strings.
// - The normal release build (`CI=true pnpm tauri build --bundles app`) must contain none.
// - `cargo tree` must not list tauri-plugin-wdio-webdriver without the `e2e` feature.
//
// Usage: node e2e/check-release.mjs [release-binary] [e2e-binary]
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const tauriDir = path.join(here, "..", "src-tauri");
const release =
  process.argv[2] ?? path.join(tauriDir, "target/release/bundle/macos/Vice Mail Spike.app/Contents/MacOS/vice-mail-spike");
const e2e = process.argv[3] ?? path.join(tauriDir, "target/debug/vice-mail-spike");

// Strings that only exist when the plugin or the e2e helpers are compiled in.
const MARKERS = ["TAURI_WEBDRIVER_PORT", "wdio-webdriver", "WDIO WebDriver plugin initialized", "e2e_native_mouse", "wdioEvalResult"];

function markersIn(file) {
  if (!fs.existsSync(file)) throw new Error(`missing binary: ${file}`);
  const bytes = fs.readFileSync(file);
  return MARKERS.filter((m) => bytes.includes(Buffer.from(m)));
}

let ok = true;
const e2eFound = markersIn(e2e);
console.log(`e2e build     ${e2e}\n  markers: ${e2eFound.join(", ") || "none"}`);
if (e2eFound.length !== MARKERS.length) {
  console.log("  FAIL: positive control - the e2e build must contain every marker");
  ok = false;
}

const releaseFound = markersIn(release);
console.log(`release build ${release}\n  markers: ${releaseFound.join(", ") || "none"}`);
if (releaseFound.length > 0) {
  console.log("  FAIL: release build contains WebDriver/e2e code");
  ok = false;
}

const tree = (features) => {
  try {
    return execFileSync("cargo", ["tree", "-e", "normal", "-i", "tauri-plugin-wdio-webdriver", ...features], {
      cwd: tauriDir,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch (e) {
    return `ERR ${String(e.stderr ?? e.message).trim().split("\n")[0]}`;
  }
};
const withoutFeature = tree([]);
const withFeature = tree(["--features", "e2e"]);
console.log(`cargo tree (default):      ${withoutFeature.trim().split("\n")[0]}`);
console.log(`cargo tree (--features e2e): ${withFeature.trim().split("\n")[0]}`);
if (!withoutFeature.startsWith("ERR") || withFeature.startsWith("ERR")) {
  console.log("  FAIL: dependency graph does not match (plugin only with --features e2e)");
  ok = false;
}

console.log(ok ? "OK: embedded WebDriver only in the e2e build" : "FAILED");
process.exit(ok ? 0 : 1);
