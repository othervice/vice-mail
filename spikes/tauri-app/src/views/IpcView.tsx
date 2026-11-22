import { useEffect, useRef, useState, type ReactElement } from "react";
import { ipc, type CoreEvent, type MessageDetail } from "../services/ipc";
import { ipcTsRs } from "../services/ipc-tsrs";

type Check = { name: string; pass: boolean; info: string };

const QUERY = { accountId: "acc-1", uid: 42, includeBody: true } as const;
const MOD_SEQ = "9007199254740993"; // 2^53 + 1, see commands.rs

function waitForSync(events: CoreEvent[], timeoutMs: number): Promise<boolean> {
  return new Promise((resolve) => {
    const started = Date.now();
    const tick = (): void => {
      if (events.some((e) => e.type === "messagesChanged")) resolve(true);
      else if (Date.now() - started > timeoutMs) resolve(false);
      else setTimeout(tick, 25);
    };
    tick();
  });
}

async function runSelfTest(): Promise<{ checks: Check[]; message: MessageDetail | null }> {
  const checks: Check[] = [];
  const add = (name: string, pass: boolean, info = ""): void => {
    checks.push({ name, pass, info });
  };

  // 1. typed result (tauri-specta)
  const res = await ipc.getMessage(QUERY);
  const message = res.ok ? res.value : null;
  add("specta: getMessage ok", res.ok, res.ok ? "" : res.error.message);
  if (res.ok) {
    const m = res.value;
    add("modSeq is exact string", m.modSeq === MOD_SEQ, m.modSeq);
    add("sizeBytes is number", typeof m.sizeBytes === "number", String(m.sizeBytes));
    add("sentAt is RFC 3339 string", m.sentAt === "2026-11-20T09:30:00Z", String(m.sentAt));
    add(
      "body is tagged enum",
      m.body.kind === "html" && m.body.blockedRemoteImages === 2,
      m.body.kind,
    );
    add("flags carry data", m.flags.some((f) => f.kind === "keyword" && f.value === "$Label1"));
  }
  add(
    "JSON number would lose 2^53+1",
    String(JSON.parse(MOD_SEQ) as number) !== MOD_SEQ,
    String(JSON.parse(MOD_SEQ)),
  );

  // 2. typed error
  const err = await ipc.getMessage({ ...QUERY, uid: 0 });
  add(
    "specta: typed error",
    !err.ok && err.error.code === "not_found" && typeof err.error.detail === "string",
    err.ok ? "unexpected ok" : `${err.error.code}: ${err.error.message}`,
  );

  // 3. framework error (bad argument) is not an IpcError and must be mapped
  const probe = await ipc.probeFrameworkError();
  add("framework error mapped to internal", probe.error.code === "internal", `raw ${probe.rawType}: ${probe.error.detail ?? ""}`);

  // 4. ts-rs path returns the identical payload
  const viaTsRs = await ipcTsRs.getMessage(QUERY);
  add(
    "ts-rs: same payload",
    viaTsRs.ok && JSON.stringify(viaTsRs.value) === JSON.stringify(message),
    viaTsRs.ok ? "" : viaTsRs.error.message,
  );

  // 5. typed events, both listeners
  const viaSpecta: CoreEvent[] = [];
  const viaTsRsEvents: CoreEvent[] = [];
  const un1 = await ipc.onCoreEvent((e) => viaSpecta.push(e));
  const un2 = await ipcTsRs.onCoreEvent((e) => viaTsRsEvents.push(e));
  const started = await ipc.startDemoSync("acc-1");
  add("startDemoSync ok", started.ok, started.ok ? "" : started.error.message);
  const done = await waitForSync(viaSpecta, 5000);
  un1();
  un2();
  const progress = viaSpecta.filter((e) => e.type === "syncProgress").length;
  add("specta: events arrived", done && progress === 5, `${viaSpecta.length} events, ${progress} progress`);
  add("ts-rs: events arrived", viaTsRsEvents.length === viaSpecta.length, `${viaTsRsEvents.length} events`);

  return { checks, message };
}

export function IpcView(): ReactElement {
  const [checks, setChecks] = useState<Check[]>([]);
  const [message, setMessage] = useState<MessageDetail | null>(null);
  const started = useRef(false);

  useEffect(() => {
    if (started.current) return; // StrictMode runs effects twice in dev
    started.current = true;
    void (async () => {
      let ok = false;
      let summary = "";
      try {
        const result = await runSelfTest();
        setChecks(result.checks);
        setMessage(result.message);
        ok = result.checks.every((c) => c.pass);
        summary = result.checks.map((c) => `${c.pass ? "PASS" : "FAIL"} ${c.name}${c.info ? ` (${c.info})` : ""}`).join("; ");
      } catch (e) {
        summary = `self-test threw: ${String(e)}`;
      }
      await ipc.reportFrontend(ok, summary);
    })();
  }, []);

  return (
    <section>
      <h1>IPC self-test</h1>
      <ul className="checks" data-testid="checks">
        {checks.map((c) => (
          <li key={c.name} className={c.pass ? "pass" : "fail"}>
            {c.name}
            {c.info ? ` (${c.info})` : ""}
          </li>
        ))}
      </ul>
      <h2>MessageDetail</h2>
      <pre>{message ? JSON.stringify(message, null, 2) : "…"}</pre>
    </section>
  );
}
