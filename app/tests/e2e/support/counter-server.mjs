// Counting HTTP server for the reading test (reading.e2e.mjs), after Spike D (decision 0005).
// Every TCP connection and every request on the counted ports is recorded, with the path for
// requests, so a leak can be traced to the vector that caused it. The hostile corpus points all
// its external references here (testmail/corpus/hostile/*.eml).
//
//   counted:  http://127.0.0.1:18999/…   (every vector, the approved images)
//             http://127.0.0.1:18997/…   (only <link rel=preconnect>, which opens a bare connection)
//   control:  GET  http://127.0.0.1:18998/hits    -> { connections, requests }
//             POST http://127.0.0.1:18998/reset
//
// The control port never counts. Answers carry `Connection: close`, and idle sockets are closed
// after 500 ms, so every later access has to open a new, countable connection.
import http from "node:http";

export const COUNTED_PORTS = [18999, 18997];
export const CONTROL_PORT = 18998;
const HOST = "127.0.0.1";

/** A 1×1 PNG: the core only accepts raster images it recognises by their bytes. */
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64",
);

function body(path) {
  if (path.endsWith(".css")) return ["text/css", Buffer.from("/* counted */")];
  if (path.endsWith(".js")) return ["text/javascript", Buffer.from("/* counted */")];
  if (/\.(html?)$/.test(path)) return ["text/html", Buffer.from("<!doctype html><title>counted</title>counted")];
  return ["image/png", PNG];
}

/** Starts the server in this process; `stop()` closes it. */
export async function startCounter() {
  const state = { connections: [], requests: [] };
  const servers = [];

  for (const port of COUNTED_PORTS) {
    const server = http.createServer((req, res) => {
      state.requests.push({
        port,
        method: req.method,
        path: req.url ?? "",
        time: Date.now(),
        userAgent: req.headers["user-agent"] ?? null,
        referer: req.headers.referer ?? null,
        cookie: req.headers.cookie ?? null,
        origin: req.headers.origin ?? null,
      });
      const [type, data] = body(req.url ?? "");
      res.writeHead(200, { "Content-Type": type, "Content-Length": data.length, Connection: "close", "Cache-Control": "no-store" });
      res.end(data);
    });
    server.on("connection", (socket) => {
      state.connections.push({ port, time: Date.now() });
      // A speculative connection (preconnect) kept open would hide every later one.
      socket.setTimeout(500, () => socket.destroy());
    });
    servers.push(server);
  }

  const control = http.createServer((req, res) => {
    if (req.method === "POST" && req.url === "/reset") {
      state.connections = [];
      state.requests = [];
      res.writeHead(204).end();
    } else if (req.method === "GET" && req.url === "/hits") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify(state));
    } else {
      res.writeHead(404).end();
    }
  });
  servers.push(control);

  const ports = [...COUNTED_PORTS, CONTROL_PORT];
  await Promise.all(
    servers.map(
      (s, i) =>
        new Promise((resolve, reject) => {
          s.once("error", (e) => reject(new Error(`counter server: cannot listen on ${HOST}:${ports[i]} (${e.code ?? e.message})`)));
          s.listen(ports[i], HOST, resolve);
        }),
    ),
  );
  return {
    stop: () =>
      Promise.all(
        servers.map(
          (s) =>
            new Promise((resolve) => {
              s.closeAllConnections?.();
              s.close(() => resolve());
            }),
        ),
      ),
  };
}

/** Client side, through the control port. */
export const counter = {
  async reset() {
    const r = await fetch(`http://${HOST}:${CONTROL_PORT}/reset`, { method: "POST" });
    if (!r.ok) throw new Error(`counter reset failed: ${r.status}`);
  },
  /** `{ connections: [{port, time}], requests: [{port, path, …}] }` since the last reset. */
  async hits() {
    const r = await fetch(`http://${HOST}:${CONTROL_PORT}/hits`);
    if (!r.ok) throw new Error(`counter query failed: ${r.status}`);
    return r.json();
  },
};
