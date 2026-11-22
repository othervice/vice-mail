// Counting HTTP server for Spike D. Every TCP connection and every request on the counted
// ports is recorded with its path, so a leak can be traced to the vector that caused it.
// The control port is separate and never counts.
//
//   counted:  http://127.0.0.1:18999/…   (all vectors and controls)
//             http://127.0.0.1:18997/…   (only <link rel=preconnect>, which opens a bare connection)
//   control:  GET  http://127.0.0.1:18998/hits    -> { connections, requests }
//             POST http://127.0.0.1:18998/reset
//
// Standalone: `node e2e/counter-server.mjs` (Ctrl+C to stop).
import http from "node:http";
import { pathToFileURL } from "node:url";

export const COUNTED_PORTS = [18999, 18997];
export const CONTROL_PORT = 18998;
const HOST = "127.0.0.1";

const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64",
);

function body(path) {
  if (/\.(png|gif|jpe?g|webp)$/.test(path)) return ["image/png", PNG];
  if (path.endsWith(".css")) return ["text/css", Buffer.from("/* counted */")];
  if (path.endsWith(".js")) return ["text/javascript", Buffer.from("/* counted */")];
  if (path.endsWith(".svg")) return ["image/svg+xml", Buffer.from('<svg xmlns="http://www.w3.org/2000/svg" id="x"/>')];
  return ["text/html", Buffer.from("<!doctype html><title>counted</title>counted")];
}

export async function startCounter({ quiet = false } = {}) {
  const state = { connections: [], requests: [] };
  const servers = [];

  for (const port of COUNTED_PORTS) {
    const server = http.createServer((req, res) => {
      const entry = {
        port,
        method: req.method,
        path: req.url ?? "",
        time: new Date().toISOString(),
        userAgent: req.headers["user-agent"] ?? null,
        referer: req.headers.referer ?? null,
        origin: req.headers.origin ?? null,
      };
      state.requests.push(entry);
      if (!quiet) console.log(`[counter] ${port} ${entry.method} ${entry.path}`);
      const [type, data] = body(entry.path);
      // No keep-alive: a leak after a control must open a new, countable connection.
      res.writeHead(200, { "Content-Type": type, "Content-Length": data.length, Connection: "close", "Cache-Control": "no-store" });
      res.end(data);
    });
    server.on("connection", (socket) => {
      state.connections.push({ port, time: new Date().toISOString() });
      // Close idle sockets quickly: a speculative connection (preconnect) kept open by WebKit
      // would otherwise hide every later one.
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
          s.once("error", (e) =>
            reject(new Error(`counter server: cannot listen on ${HOST}:${ports[i]} (${e.code ?? e.message})`)),
          );
          s.listen(ports[i], HOST, resolve);
        }),
    ),
  );
  if (!quiet) console.log(`[counter] counting on ${COUNTED_PORTS.join(", ")}, control on ${CONTROL_PORT}`);

  return {
    stop: () => Promise.all(servers.map((s) => new Promise((resolve) => s.close(() => resolve())))),
  };
}

/** Client side, used by the test workers. */
export const counter = {
  async reset() {
    const r = await fetch(`http://${HOST}:${CONTROL_PORT}/reset`, { method: "POST" });
    if (!r.ok) throw new Error(`counter reset failed: ${r.status}`);
  },
  async hits() {
    const r = await fetch(`http://${HOST}:${CONTROL_PORT}/hits`);
    if (!r.ok) throw new Error(`counter query failed: ${r.status}`);
    return r.json();
  },
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const c = await startCounter();
  process.on("SIGINT", () => c.stop().then(() => process.exit(0)));
}
