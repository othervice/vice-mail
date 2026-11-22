// Spike D: hostile HTML snippets. Each vector points at its own path on the local counter
// server (`/v/<id>/…`), so any request can be traced back to the vector that caused it.
// Script vectors additionally try to report themselves without the network (postMessage to the
// parent, markers on their own and the parent window, the Tauri interface).
//
// Stage 3 builds the full corpus under `testmail/corpus/hostile/`; this is the spike subset.

export const COUNTER = "http://127.0.0.1:18999";
/** Second counted port, only for `<link rel=preconnect>` (a bare TCP connect has no path). */
export const COUNTER_PRECONNECT = "http://127.0.0.1:18997";

export type Interaction = "none" | "click" | "hover";

export type VectorKind = "script" | "network" | "link" | "render";

export type Vector = {
  id: string;
  kind: VectorKind;
  /** What the test does with the frame after load. */
  interaction: Interaction;
  html: string;
  /** For `link` vectors: the real target the app must learn instead of navigating. */
  target?: string;
};

const p = (id: string, file: string): string => `${COUNTER}/v/${id}/${file}`;

/**
 * JavaScript a hostile vector tries to run. Only single quotes and no semicolons, so the same
 * text fits into attributes, `javascript:` URLs and `meta refresh`.
 */
export function js(id: string): string {
  return [
    `try{parent.postMessage({vmRan:'${id}'},'*')}catch(e){}`,
    `try{window.__vmRan='${id}'}catch(e){}`,
    `try{top.document.title='PWNED ${id}'}catch(e){}`,
    `try{window.__TAURI_INTERNALS__.invoke('sandbox_breach',{vector:'${id}'})}catch(e){}`,
    `try{parent.__TAURI_INTERNALS__.invoke('sandbox_breach',{vector:'${id}'})}catch(e){}`,
    `try{fetch('${p(id, "js-fetch")}')}catch(e){}`,
    `new Image().src='${p(id, "js-img.png")}'`,
  ].join("");
}

const PIXEL = "data:image/gif;base64,R0lGODlhAQABAAAAACw=";

function v(id: string, kind: VectorKind, interaction: Interaction, html: string, target?: string): Vector {
  return target === undefined ? { id, kind, interaction, html } : { id, kind, interaction, html, target };
}

export const VECTORS: readonly Vector[] = [
  // --- scripts -------------------------------------------------------------------------------
  v("script-inline", "script", "none", `<script>${js("script-inline")}</script>`),
  v("script-src-self", "script", "none", `<script src="/sandbox-probe.js?v=script-src-self"></script>`),
  v("script-src-remote", "script", "none", `<script src="${p("script-src-remote", "x.js")}"></script>`),
  v("img-onerror", "script", "none", `<img src="data:," onerror="${js("img-onerror")}">`),
  v("img-onload", "script", "none", `<img src="${PIXEL}" onload="${js("img-onload")}">`),
  v("body-onload", "script", "none", `<body onload="${js("body-onload")}">body</body>`),
  v("details-ontoggle", "script", "none", `<details open ontoggle="${js("details-ontoggle")}"><summary>d</summary>x</details>`),
  v("input-autofocus", "script", "none", `<input autofocus onfocus="${js("input-autofocus")}">`),
  v("onmouseover", "script", "hover", `<div onmouseover="${js("onmouseover")}">HOVER HOVER HOVER HOVER</div>`),
  v("javascript-link", "script", "click", `<a href="javascript:${js("javascript-link")}">js link</a>`),
  v("javascript-iframe", "script", "none", `<iframe src="javascript:${js("javascript-iframe")}"></iframe>`),
  v("svg-script", "script", "none", `<svg><script>${js("svg-script")}</script></svg>`),
  v("svg-onload", "script", "none", `<svg onload="${js("svg-onload")}"><rect width="5" height="5"/></svg>`),
  v(
    "svg-animate-href",
    "script",
    "click",
    `<svg width="200" height="30"><a><animate attributeName="href" values="javascript:${js("svg-animate-href")}"/><text x="0" y="15">svg anim</text></a></svg>`,
  ),
  v("meta-refresh-js", "script", "none", `<meta http-equiv="refresh" content="0;url=javascript:${js("meta-refresh-js")}">`),
  v("iframe-srcdoc-nested", "script", "none", `<iframe srcdoc="&lt;script&gt;${js("iframe-srcdoc-nested")}&lt;/script&gt;"></iframe>`),
  v("iframe-app-self", "script", "none", `<iframe src="/index.html"></iframe>`),

  // --- passive network ------------------------------------------------------------------------
  v("img-src", "network", "none", `<img src="${p("img-src", "a.png")}">`),
  v("img-srcset", "network", "none", `<img srcset="${p("img-srcset", "a.png")} 1x, ${p("img-srcset", "b.png")} 2x">`),
  v("picture-source", "network", "none", `<picture><source srcset="${p("picture-source", "a.png")}"><img alt="p"></picture>`),
  v("video-poster", "network", "none", `<video poster="${p("video-poster", "a.png")}"></video>`),
  v("video-src", "network", "none", `<video src="${p("video-src", "a.mp4")}" preload="auto"></video>`),
  v("audio-src", "network", "none", `<audio src="${p("audio-src", "a.mp3")}" preload="auto" controls></audio>`),
  v("input-image", "network", "none", `<input type="image" src="${p("input-image", "a.png")}" alt="i">`),
  v("svg-image", "network", "none", `<svg width="10" height="10"><image href="${p("svg-image", "a.png")}" width="10" height="10"/></svg>`),
  v("svg-use", "network", "none", `<svg width="10" height="10"><use href="${p("svg-use", "a.svg")}#x"/></svg>`),
  v("css-style-attr", "network", "none", `<div style="background-image:url(${p("css-style-attr", "a.png")})">bg</div>`),
  v("css-style-block", "network", "none", `<style>.b{background:url(${p("css-style-block", "a.png")})}</style><div class="b">bg</div>`),
  v("css-import", "network", "none", `<style>@import url(${p("css-import", "a.css")});</style>imp`),
  v(
    "css-font-face",
    "network",
    "none",
    `<style>@font-face{font-family:vmx;src:url(${p("css-font-face", "a.woff")})}body{font-family:vmx}</style>font`,
  ),
  v("link-stylesheet", "network", "none", `<link rel="stylesheet" href="${p("link-stylesheet", "a.css")}">ls`),
  v("link-prefetch", "network", "none", `<link rel="prefetch" href="${p("link-prefetch", "a")}">pf`),
  v("link-preload", "network", "none", `<link rel="preload" as="image" href="${p("link-preload", "a.png")}">pl`),
  v(
    "link-preconnect",
    "network",
    "none",
    `<link rel="preconnect" href="${COUNTER_PRECONNECT}"><link rel="dns-prefetch" href="//dns-prefetch.vector.invalid">pc`,
  ),
  v("body-background", "network", "none", `<body background="${p("body-background", "a.png")}">bb</body>`),
  v("table-background", "network", "none", `<table background="${p("table-background", "a.png")}"><tr><td>tb</td></tr></table>`),
  v("object-data", "network", "none", `<object data="${p("object-data", "a.html")}"></object>`),
  v("embed-src", "network", "none", `<embed src="${p("embed-src", "a.swf")}">`),
  v("iframe-src", "network", "none", `<iframe src="${p("iframe-src", "a.html")}"></iframe>`),
  v("base-href", "network", "none", `<base href="${COUNTER}/v/base-href/"><img src="relative.png">`),
  v("meta-refresh", "network", "none", `<meta http-equiv="refresh" content="0;url=${p("meta-refresh", "a")}">`),
  v(
    "form-action",
    "network",
    "click",
    `<form action="${p("form-action", "submit")}" method="post"><button type="submit">send form</button><input name="a" value="b"></form>`,
  ),

  // --- links ----------------------------------------------------------------------------------
  v("link-plain", "link", "click", `<a href="${p("link-plain", "a")}">plain link</a>`, p("link-plain", "a")),
  v("link-target-top", "link", "click", `<a href="${p("link-target-top", "a")}" target="_top">top link</a>`, p("link-target-top", "a")),
  v("link-target-blank", "link", "click", `<a href="${p("link-target-blank", "a")}" target="_blank">blank link</a>`, p("link-target-blank", "a")),
  v("link-ping", "link", "click", `<a href="${p("link-ping", "a")}" ping="${p("link-ping", "ping")}">ping link</a>`, p("link-ping", "a")),
  v("link-download", "link", "click", `<a href="${p("link-download", "a")}" download>download link</a>`, p("link-download", "a")),
  v("svg-link", "link", "click", `<svg width="200" height="30"><a href="${p("svg-link", "a")}"><text x="0" y="15">svg link</text></a></svg>`, p("svg-link", "a")),
  v("link-mailto", "link", "click", `<a href="mailto:someone@example.test?subject=spike">mailto link</a>`, "mailto:someone@example.test?subject=spike"),
  // What a sanitizer could rewrite every href to: an app-internal scheme the Rust hook resolves.
  v("link-custom-scheme", "link", "click", `<a href="vmlink:42">custom scheme link</a>`, "vmlink:42"),
  v("link-rewritten-https", "link", "click", `<a href="https://link.invalid/42">rewritten link</a>`, "https://link.invalid/42"),

  // --- rendering (informational: does mail CSS still apply?) ----------------------------------
  v("style-block", "render", "none", `<style>#probe{color:rgb(1,2,3)}</style><p id="probe">style block</p>`),
  v("style-attr", "render", "none", `<p id="probe" style="color:rgb(4,5,6)">style attribute</p>`),
];
