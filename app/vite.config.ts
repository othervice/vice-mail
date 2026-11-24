import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// Tauri loads the dev server from a fixed port (devUrl) and must keep the Rust output visible.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: "localhost" },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
    // Stage 18 (decision 0051): the new-mail popup's window loads a page of its own, without the
    // rest of the app.
    rollupOptions: { input: { main: "index.html", popup: "popup.html" } },
  },
  test: {
    environment: "jsdom",
    include: ["tests/**/*.test.{ts,tsx}"],
    setupFiles: ["./tests/setup.ts"],
    restoreMocks: true,
    // The axe and focus tests take 1–2 s alone and over 5 s on a busy build machine or CI host
    // (stage 17: three timeouts at load 18). A hang still fails, after 15 s.
    testTimeout: 15_000,
    // Vitest blanks CSS imports by default; the token tests read tokens.css as raw text.
    css: { include: [/\.css(\?.*)?$/] },
  },
});
