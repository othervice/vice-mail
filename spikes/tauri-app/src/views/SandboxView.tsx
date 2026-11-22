// Spike D: renders hostile HTML in sandboxed iframes and records everything a leak would
// leave behind. The e2e test (e2e/sandbox.e2e.mjs) drives this view; it can also be used by hand.
import { useEffect, useRef, useState, type ReactElement } from "react";
import { ipc, type SandboxEvent, type SandboxReport } from "../services/ipc";
import { SETS, framesFor, srcdoc, type FrameSpec, type SetId } from "../sandbox/frames";

type Intercept = { frame: string; href: string; target: string | null; trusted: boolean; type: string };

type Observed = {
  messages: { frame: string | null; data: unknown; origin: string }[];
  intercepts: Intercept[];
  hovers: { frame: string; trusted: boolean }[];
  submits: { frame: string }[];
  loaded: string[];
  rustEvents: SandboxEvent[];
};

type FrameInspection = {
  frame: string;
  accessible: boolean;
  href?: string;
  vmRan?: string;
  title?: string;
  tauriInternals?: string;
  isTauri?: string;
  wryIpc?: string;
  color?: string;
  /** Content height as the parent can measure it (auto-sizing the frame needs this). */
  height?: number;
  nested?: string[];
  error?: string;
};

export type SandboxSnapshot = {
  set: SetId | null;
  run: number;
  frames: number;
  specs: Omit<FrameSpec, "html">[];
  observed: Observed;
  inspections: FrameInspection[];
  rust: SandboxReport | null;
  rustError: string | null;
  mainHref: string;
  title: string;
  cspAttributeSupported: boolean;
};

declare global {
  interface Window {
    __vmSandbox?: {
      snapshot: () => Promise<SandboxSnapshot>;
      clear: () => Promise<void>;
    };
  }
}

const XLINK = "http://www.w3.org/1999/xlink";
const MAX_HOVERS = 200;

function emptyObserved(): Observed {
  return { messages: [], intercepts: [], hovers: [], submits: [], loaded: [], rustEvents: [] };
}

/** Raw link target of an `<a>`/`<area>` (HTML or SVG), without letting the frame resolve it. */
function rawHref(el: Element): string {
  const attr = el.getAttribute("href") ?? el.getAttributeNS(XLINK, "href");
  if (attr !== null) return attr;
  const anim = (el as unknown as { href?: { animVal?: unknown } }).href?.animVal;
  return typeof anim === "string" ? anim : "";
}

function str(value: unknown): string | undefined {
  return value === undefined ? undefined : String(value);
}

function inspect(spec: FrameSpec, el: HTMLIFrameElement | undefined): FrameInspection {
  if (!el) return { frame: spec.id, accessible: false, error: "frame element missing" };
  try {
    const w = el.contentWindow as (Window & Record<string, unknown>) | null;
    const d = el.contentDocument;
    if (!w || !d) return { frame: spec.id, accessible: false };
    const probe = d.getElementById("probe");
    const nested = Array.from(d.querySelectorAll("iframe")).map((f) => {
      try {
        const nw = f.contentWindow as (Window & Record<string, unknown>) | null;
        if (!nw || !f.contentDocument) return "opaque";
        return `href=${nw.location.href} tauri=${typeof nw["__TAURI_INTERNALS__"]} ran=${String(nw["__vmRan"])}`;
      } catch (e) {
        return `error ${String(e)}`;
      }
    });
    const result: FrameInspection = {
      frame: spec.id,
      accessible: true,
      href: w.location.href,
      title: d.title,
      tauriInternals: typeof w["__TAURI_INTERNALS__"],
      isTauri: typeof w["isTauri"],
      wryIpc: typeof w["ipc"],
      nested,
      height: d.documentElement.scrollHeight,
    };
    const ran = str(w["__vmRan"]);
    if (ran !== undefined) result.vmRan = ran;
    if (probe) result.color = w.getComputedStyle(probe).color;
    return result;
  } catch (e) {
    return { frame: spec.id, accessible: false, error: String(e) };
  }
}

export function SandboxView(): ReactElement {
  const [set, setSet] = useState<SetId | null>(null);
  const [run, setRun] = useState(0);
  const [, setVersion] = useState(0);
  const observed = useRef<Observed>(emptyObserved());
  const frames = useRef(new Map<string, HTMLIFrameElement>());
  const specs = set === null ? [] : framesFor(set);
  const specsRef = useRef<FrameSpec[]>(specs);
  specsRef.current = specs;
  const bump = (): void => setVersion((n) => n + 1);

  // postMessage from frames: only a running script can send one.
  useEffect(() => {
    const onMessage = (e: MessageEvent): void => {
      let frame: string | null = null;
      for (const [id, el] of frames.current) if (el.contentWindow === e.source) frame = id;
      observed.current.messages.push({ frame, data: e.data as unknown, origin: e.origin });
      bump();
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, []);

  // Navigations the Rust hook blocked (or, in e2e builds, allowed under /allowed/).
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void ipc.onSandboxEvent((ev) => {
      observed.current.rustEvents.push(ev);
      bump();
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Test hook. The e2e test reads everything through this one call.
  useEffect(() => {
    window.__vmSandbox = {
      snapshot: async () => {
        let rust: SandboxReport | null = null;
        let rustError: string | null = null;
        try {
          rust = await ipc.sandboxReport();
        } catch (e) {
          rustError = String(e);
        }
        return {
          set,
          run,
          frames: specsRef.current.length,
          specs: specsRef.current.map(({ html: _html, ...rest }) => rest),
          observed: structuredClone(observed.current),
          inspections: specsRef.current.map((s) => inspect(s, frames.current.get(s.id))),
          rust,
          rustError,
          mainHref: window.location.href,
          title: document.title,
          cspAttributeSupported: "csp" in HTMLIFrameElement.prototype,
        };
      },
      clear: async () => {
        observed.current = emptyObserved();
        await ipc.sandboxReset();
        bump();
      },
    };
    return () => {
      delete window.__vmSandbox;
    };
  }, [set, run]);

  const load = (id: SetId): void => {
    observed.current = emptyObserved();
    frames.current.clear();
    setSet(id);
    setRun((n) => n + 1);
  };

  const onFrameLoad = (spec: FrameSpec, el: HTMLIFrameElement): void => {
    observed.current.loaded.push(spec.id);
    bump();
    let doc: Document | null = null;
    try {
      doc = el.contentDocument;
    } catch {
      doc = null;
    }
    if (!doc) return; // opaque origin: the parent cannot see inside
    doc.addEventListener(
      "mouseover",
      (e) => {
        if (observed.current.hovers.length < MAX_HOVERS) observed.current.hovers.push({ frame: spec.id, trusted: e.isTrusted });
      },
      true,
    );
    if (!spec.parentClicks) return;
    // Variant A: intercept link clicks in the parent. Finding: WebKit never calls these listeners
    // (nor any other listener on nodes of a document without allow-scripts, even when the function
    // comes from the parent). Kept so the e2e run documents it; links are caught by the Rust hook.
    const onClick = (e: Event): void => {
      const target = e.target as Element | null;
      const link = typeof target?.closest === "function" ? target.closest("a, area") : null;
      if (!link) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      observed.current.intercepts.push({
        frame: spec.id,
        href: rawHref(link),
        target: link.getAttribute("target"),
        trusted: e.isTrusted,
        type: e.type,
      });
      bump();
    };
    doc.addEventListener("click", onClick, true);
    doc.addEventListener("auxclick", onClick, true);
    doc.addEventListener(
      "submit",
      (e) => {
        e.preventDefault();
        observed.current.submits.push({ frame: spec.id });
        bump();
      },
      true,
    );
  };

  const obs = observed.current;
  const loaded = new Set(obs.loaded).size;

  return (
    <section className="sandbox">
      <h1>HTML sandbox</h1>
      <div className="sb-sets">
        {SETS.map((s) => (
          <button key={s.id} type="button" data-testid={`sb-set-${s.id}`} title={s.description} onClick={() => load(s.id)}>
            {s.title}
          </button>
        ))}
      </div>
      <p data-testid="sb-status" data-set={set ?? ""} data-run={run} data-loaded={loaded} data-total={specs.length}>
        {set === null ? "No set loaded." : `Set ${set}, run ${run}: ${loaded}/${specs.length} frames loaded.`}{" "}
        Script messages: {obs.messages.length}. Link targets: {obs.intercepts.length + obs.rustEvents.length}.
      </p>
      <ul className="sb-targets" data-testid="sb-targets">
        {obs.intercepts.map((i, n) => (
          <li key={`p${n}`}>
            parent: {i.frame} → {i.href}
          </li>
        ))}
        {obs.rustEvents.map((ev, n) => (
          <li key={`r${n}`}>
            rust ({ev.allowed ? "allowed" : "blocked"}): {ev.url}
          </li>
        ))}
      </ul>
      <div className="sb-grid">
        {specs.map((spec) => (
          <div key={`${run}:${spec.id}`} className="sb-cell" data-frame={spec.id}>
            <span className="sb-label">{spec.id}</span>
            <iframe
              title={spec.id}
              sandbox={spec.sandbox}
              srcDoc={srcdoc(spec)}
              {...(spec.cspAttr === undefined ? {} : ({ csp: spec.cspAttr } as Record<string, string>))}
              ref={(el) => {
                if (el) frames.current.set(spec.id, el);
              }}
              onLoad={(e) => onFrameLoad(spec, e.currentTarget)}
            />
          </div>
        ))}
      </div>
    </section>
  );
}
