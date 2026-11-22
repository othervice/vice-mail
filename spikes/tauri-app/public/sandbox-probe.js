// Spike D probe. Loaded from the app origin ('self') by script vectors and by the positive
// controls. In a frame without `allow-scripts` it must never run. When it runs (controls with
// `allow-scripts`), it reports what a script inside a mail frame could see and reach.
(function () {
  var v = "unknown";
  try {
    v = new URL(document.currentScript.src).searchParams.get("v") || v;
  } catch (e) {}
  var t = function (f) {
    try {
      return f();
    } catch (e) {
      return "error: " + (e && e.name ? e.name : String(e));
    }
  };
  var info = {
    vmRan: v,
    probe: true,
    origin: t(function () { return String(self.origin); }),
    href: t(function () { return String(location.href); }),
    tauriInternals: t(function () { return typeof window.__TAURI_INTERNALS__; }),
    isTauri: t(function () { return typeof window.isTauri; }),
    wryIpc: t(function () { return typeof window.ipc; }),
    webkitIpcHandler: t(function () {
      return typeof (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.ipc);
    }),
    parentDocument: t(function () { return typeof parent.document; }),
    parentTauriInternals: t(function () { return typeof parent.__TAURI_INTERNALS__; }),
  };
  try { window.__vmRan = v; } catch (e) {}
  try { parent.postMessage(info, "*"); } catch (e) {}
  // Breach attempts. Each one that succeeds shows up in the Rust breach log.
  try {
    if (window.__TAURI_INTERNALS__) window.__TAURI_INTERNALS__.invoke("sandbox_breach", { vector: v + "#own-internals" });
  } catch (e) {}
  try {
    if (parent.__TAURI_INTERNALS__) parent.__TAURI_INTERNALS__.invoke("sandbox_breach", { vector: v + "#via-parent" });
  } catch (e) {}
  // Raw IPC request without the Tauri runtime script (no invoke key): must be rejected.
  try {
    fetch("ipc://localhost/sandbox_breach", {
      method: "POST",
      body: JSON.stringify({ vector: v + "#raw-ipc" }),
      headers: { "Content-Type": "application/json", "Tauri-Callback": "1", "Tauri-Error": "2", "Tauri-Invoke-Key": "guess" },
    }).then(
      function (r) { parent.postMessage({ vmRan: v, rawIpc: "status " + r.status }, "*"); },
      function (e) { parent.postMessage({ vmRan: v, rawIpc: "rejected: " + String(e) }, "*"); },
    );
  } catch (e) {
    parent.postMessage({ vmRan: v, rawIpc: "threw: " + String(e) }, "*");
  }
  try {
    if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.ipc) {
      window.webkit.messageHandlers.ipc.postMessage(
        JSON.stringify({ cmd: "sandbox_breach", callback: 1, error: 2, payload: { vector: v + "#webkit-handler" }, options: {} }),
      );
    }
  } catch (e) {}
  try { fetch("http://127.0.0.1:18999/v/" + v + "/probe-fetch"); } catch (e) {}
  // Pointer detector for the controls: did a (native) click reach an element in this frame?
  ["mouseover", "mousedown", "click"].forEach(function (type) {
    document.addEventListener(type, function (e) {
      parent.postMessage({ vmRan: v, event: type, trusted: e.isTrusted }, "*");
    }, true);
  });
})();
