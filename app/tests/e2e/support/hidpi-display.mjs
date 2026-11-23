// A virtual HiDPI display (support/hidpi-display.m) for readme-shots.e2e.mjs, so the README
// screenshots come out at 2x on a Mac whose screen is 1x (a headless Mac mini). macOS only; Node
// only. The helper is compiled with the system's clang into the test data directory.
import { execFileSync, spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const SOURCE = path.join(here, "hidpi-display.m");

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function displays(binary) {
  return execFileSync(binary, ["--list"], { encoding: "utf8" })
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

/** Waits until no display of the helper is online any more; false after `timeoutMs`. */
async function gone(binary, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (!displays(binary).some((d) => d.ours)) return true;
    await sleep(500);
  }
  return false;
}

/**
 * Creates a display of `width` x `height` points at 2x and resolves with its bounds (global
 * coordinates, points) and `stop()`, which removes it again. While it exists, `caffeinate` keeps
 * the physical display awake: macOS removes a virtual display only while it is awake.
 */
export async function startHiDpiDisplay({ width, height, dir }) {
  const binary = path.join(dir, "hidpi-display");
  execFileSync("/usr/bin/clang", ["-fobjc-arc", "-framework", "Foundation", "-framework", "CoreGraphics", "-o", binary, SOURCE], {
    stdio: ["ignore", "ignore", "pipe"],
  });
  const awake = spawn("/usr/bin/caffeinate", ["-d", "-u", "-t", "7200"], { stdio: "ignore" });
  // One left from an earlier run that ended while the display slept: it goes once awake.
  if (!(await gone(binary, 15000))) {
    awake.kill();
    throw new Error(`a virtual display from an earlier run is still online: ${JSON.stringify(displays(binary))}`);
  }
  const child = spawn(binary, [String(width), String(height)], { stdio: ["pipe", "pipe", "pipe"] });
  let stderr = "";
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  const bounds = await new Promise((resolve, reject) => {
    let out = "";
    const timer = setTimeout(() => reject(new Error(`the virtual display did not come up: ${stderr}`)), 20000);
    child.stdout.on("data", (chunk) => {
      out += chunk;
      const line = out.split("\n")[0];
      if (out.includes("\n")) {
        clearTimeout(timer);
        resolve(JSON.parse(line));
      }
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`the display helper ended with ${String(code)}: ${stderr}`));
    });
  });
  const exited = new Promise((resolve) => child.once("exit", resolve));
  return {
    bounds,
    /** Removes the display; resolves with whether it is gone (false: it stays until the display wakes). */
    async stop() {
      child.stdin.end();
      const ended = await Promise.race([exited.then(() => true), sleep(10000).then(() => false)]);
      if (!ended) child.kill();
      const removed = await gone(binary, 20000);
      awake.kill();
      return removed;
    },
  };
}
