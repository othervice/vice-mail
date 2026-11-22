// Slows the local test server down so the app's first sync lasts long enough to scroll during it
// (account-sync.e2e.mjs). It lets the server process run for RUN_MS, pauses it (SIGSTOP) for
// PAUSE_MS, and so on: the app's connections stay open and receive the same data in bursts, as
// over a slow network. Started with `fork` from the spec's worker; resumes the server and exits
// on "stop", on SIGTERM and when the worker goes away (the IPC channel closes), so a failed run
// never leaves the server frozen.
//
// Usage: node throttle-server.mjs PID RUN_MS PAUSE_MS
const [pid, runMs, pauseMs] = process.argv.slice(2).map(Number);
if (!Number.isInteger(pid) || !(runMs > 0) || !(pauseMs > 0)) {
  console.error("usage: node throttle-server.mjs PID RUN_MS PAUSE_MS");
  process.exit(2);
}

const signal = (name) => {
  try {
    process.kill(pid, name);
  } catch {
    // The server is gone; nothing to pause or resume.
  }
};

const finish = () => {
  signal("SIGCONT");
  process.exit(0);
};
process.on("message", (message) => {
  if (message === "stop") finish();
});
process.on("disconnect", finish);
process.on("SIGTERM", finish);
process.on("SIGINT", finish);

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
for (;;) {
  signal("SIGCONT");
  await sleep(runMs);
  signal("SIGSTOP");
  await sleep(pauseMs);
}
