// Spike D: which frames the Sandbox view renders. One frame per vector, so every leak, click
// and navigation can be attributed. A "set" is what the e2e test loads at a time.

import { COUNTER, VECTORS, js, type Interaction, type VectorKind } from "./corpus";

/** CSP placed as `<meta http-equiv>` into every `srcdoc` that uses one. */
export const FRAME_CSP =
  "default-src 'none'; img-src data:; style-src 'unsafe-inline'; font-src data:; base-uri 'none'; form-action 'none'";

export type FrameSpec = {
  /** Unique within a set, used as `data-frame`. */
  id: string;
  vector: string;
  kind: VectorKind | "control";
  interaction: Interaction;
  /** Value of the `sandbox` attribute. */
  sandbox: string;
  metaCsp: boolean;
  /** Parent window intercepts clicks on links inside the frame (needs `allow-same-origin`). */
  parentClicks: boolean;
  html: string;
  /** Value of the (Chromium-only) `csp` attribute, if any. */
  cspAttr?: string;
  target?: string;
};

export type SetId = "A" | "B" | "N" | "controls";

export const SETS: readonly { id: SetId; title: string; description: string }[] = [
  {
    id: "A",
    title: "A: same-origin, no scripts",
    description: 'sandbox="allow-same-origin", meta CSP, parent click handler (never fires in WebKit) + Rust navigation hook',
  },
  {
    id: "B",
    title: "B: opaque, no scripts",
    description: 'sandbox="" (opaque origin), meta CSP, link clicks only seen by the Rust navigation hook',
  },
  {
    id: "N",
    title: "N: no meta CSP",
    description: 'sandbox="allow-same-origin" without meta CSP or parent handler: sandbox + inherited app CSP + Rust hook only',
  },
  { id: "controls", title: "Controls", description: "positive controls: prove that the detectors detect" },
];

function variantFrames(set: "A" | "B" | "N"): FrameSpec[] {
  const sandbox = set === "B" ? "" : "allow-same-origin";
  return VECTORS.map((vec) => {
    const base: FrameSpec = {
      id: `${set}:${vec.id}`,
      vector: vec.id,
      kind: vec.kind,
      interaction: vec.interaction,
      sandbox,
      metaCsp: set !== "N",
      parentClicks: set === "A",
      html: vec.html,
    };
    return vec.target === undefined ? base : { ...base, target: vec.target };
  });
}

const allowed = (id: string, file: string): string => `${COUNTER}/allowed/${id}/${file}`;

function control(
  vector: string,
  interaction: Interaction,
  sandbox: string,
  metaCsp: boolean,
  html: string,
  extra: Partial<FrameSpec> = {},
): FrameSpec {
  return { id: `controls:${vector}`, vector, kind: "control", interaction, sandbox, metaCsp, parentClicks: false, html, ...extra };
}

export const CONTROLS: readonly FrameSpec[] = [
  // Network: the counter counts, the inherited app CSP applies, the meta CSP applies on top.
  control("ctl-img-allowed-nometa", "none", "allow-same-origin", false, `<img src="${allowed("ctl-img-allowed-nometa", "a.png")}">`),
  control("ctl-img-allowed-opaque", "none", "", false, `<img src="${allowed("ctl-img-allowed-opaque", "a.png")}">`),
  control("ctl-img-allowed-meta", "none", "allow-same-origin", true, `<img src="${allowed("ctl-img-allowed-meta", "a.png")}">`),
  control("ctl-img-inherit", "none", "allow-same-origin", false, `<img src="${COUNTER}/ctl/ctl-img-inherit/a.png">`),
  control("ctl-csp-attr", "none", "allow-same-origin", false, `<img src="${allowed("ctl-csp-attr", "a.png")}">`, {
    cspAttr: "img-src 'none'",
  }),
  // Scripts: the detectors (postMessage, markers, breach command) notice a running script.
  control("ctl-script-probe", "none", "allow-scripts", false, `<script src="/sandbox-probe.js?v=ctl-script-probe"></script>`),
  control("ctl-script-inline", "none", "allow-scripts", false, `<script>${js("ctl-script-inline")}</script>`),
  control("ctl-script-so", "none", "allow-scripts allow-same-origin", false, `<script src="/sandbox-probe.js?v=ctl-script-so"></script>`),
  // Clicks and hover reach elements inside frames (native and WebDriver).
  control("ctl-link-native", "click", "", false, `<a href="${allowed("ctl-link-native", "a")}">allowed link</a>`),
  control("ctl-link-wd", "click", "allow-same-origin", false, `<a href="${allowed("ctl-link-wd", "a")}">allowed link</a>`),
  // Listeners of the parent never fire inside a frame without allow-scripts (WebKit), so the
  // pointer detector is the probe script itself in an allow-scripts frame.
  control("ctl-pointer", "click", "allow-scripts", false, `<script src="/sandbox-probe.js?v=ctl-pointer"></script><div>POINTER POINTER</div>`),
];

export function framesFor(set: SetId): FrameSpec[] {
  return set === "controls" ? [...CONTROLS] : variantFrames(set);
}

export function srcdoc(spec: FrameSpec): string {
  const meta = spec.metaCsp ? `<meta http-equiv="Content-Security-Policy" content="${FRAME_CSP}">` : "";
  return `<!doctype html><html><head><meta charset="utf-8">${meta}</head><body>${spec.html}</body></html>`;
}
