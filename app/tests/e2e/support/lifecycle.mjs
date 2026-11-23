// Shared by wdio.conf.mjs (launcher), the specs (workers) and check-release.mjs. Node only: no
// @wdio/globals here, the launcher imports this file.

/** e2e builds keep data and logs here and refuse to start without it (src-tauri/src/e2e.rs). */
export const DATA_DIR_ENV = "MAIL_APP_E2E_DATA_DIR";

/** Port of the WebDriver server inside the test app (tauri-plugin-wdio-webdriver's default). */
export const WEBDRIVER_PORT = 4445;

/**
 * Quits the app the way a user would, by closing its only window. Runs after WebdriverIO has
 * ended the test session (`afterSession`), so it opens a short session of its own on the
 * embedded WebDriver. Resolves once the app has exited.
 */
export async function quitApp() {
  const base = `http://127.0.0.1:${WEBDRIVER_PORT}`;
  let session;
  try {
    const res = await fetch(`${base}/session`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ capabilities: {} }),
      signal: AbortSignal.timeout(5000),
    });
    session = (await res.json()).value?.sessionId;
  } catch (e) {
    if (e?.cause?.code === "ECONNREFUSED") return; // already gone
    throw e;
  }
  if (!session) throw new Error("the embedded WebDriver refused a session for quitting the app");
  // Message and composer windows (stage 8) keep the app running when only the main window
  // closes: close them first, the main window last.
  const handles = await fetch(`${base}/session/${session}/window/handles`, { signal: AbortSignal.timeout(5000) })
    .then(async (res) => (await res.json()).value ?? [])
    .catch(() => []);
  const ordered = [...handles.filter((h) => h !== "main"), ...handles.filter((h) => h === "main")];
  for (const handle of ordered.length ? ordered : [null]) {
    if (handle !== null) {
      await fetch(`${base}/session/${session}/window`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ handle }),
        signal: AbortSignal.timeout(5000),
      }).catch(() => {});
    }
    // Closing the last window quits the app, so this request may not get an answer.
    await fetch(`${base}/session/${session}/window`, { method: "DELETE", signal: AbortSignal.timeout(5000) }).catch(() => {});
  }
  await waitForAppExit();
}

/** Resolves once nothing answers on the WebDriver port any more, i.e. the app has quit. */
export async function waitForAppExit(timeoutMs = 15000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      await fetch(`http://127.0.0.1:${WEBDRIVER_PORT}/status`, { signal: AbortSignal.timeout(1000) });
    } catch (e) {
      // Only a refused connection means "gone"; a slow answer means it is still shutting down.
      if (e?.cause?.code === "ECONNREFUSED") return;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`the app still answers on port ${WEBDRIVER_PORT} after ${timeoutMs} ms`);
}
