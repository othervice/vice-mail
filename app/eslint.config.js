// ESLint flat config. `pnpm lint` runs it with --max-warnings 0.
import js from "@eslint/js";
import i18next from "eslint-plugin-i18next";
import reactHooks from "eslint-plugin-react-hooks";
import { defineConfig } from "eslint/config";
import globals from "globals";
import tseslint from "typescript-eslint";

export default defineConfig(
  { ignores: ["dist/", "src-tauri/", "src/generated/"] },

  js.configs.recommended,
  tseslint.configs.recommendedTypeChecked,
  {
    languageOptions: {
      globals: globals.browser,
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
  },
  {
    files: ["**/*.js"],
    extends: [tseslint.configs.disableTypeChecked],
    languageOptions: { globals: globals.node },
  },

  {
    files: ["src/**/*.{ts,tsx}", "tests/**/*.{ts,tsx}"],
    extends: [reactHooks.configs.flat.recommended],
  },

  // App code: the boundaries from CLAUDE.md and docs/ARCHITECTURE.md.
  {
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/services/ipc.ts"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["@tauri-apps/api", "@tauri-apps/api/*"],
              message: "Only src/services/ipc.ts talks to the core. Use `ipc` from services/ipc.ts.",
            },
            {
              regex: "(^|/)generated(/|$)",
              message: "Only src/services/ipc.ts imports the generated bindings. Import types from services/ipc.ts.",
            },
          ],
        },
      ],
    },
  },
  {
    files: ["src/**/*.tsx"],
    plugins: { i18next },
    rules: {
      // No user-visible text in components: every string comes from src/i18n via `t`.
      // `jsx-only` covers JSX text, string attributes and string expressions inside JSX; values
      // whose type is a string-literal union (e.g. side="right") pass through the type checker.
      "i18next/no-literal-string": [
        "error",
        {
          mode: "jsx-only",
          "should-validate-template": true,
          "jsx-attributes": {
            exclude: ["className", "key", "id", "type", "role", "lang", "data-.*"],
          },
        },
      ],
      "no-restricted-syntax": [
        "error",
        {
          // The app's Content-Security-Policy has no 'unsafe-inline' for styles.
          selector: "JSXAttribute[name.name='style']",
          message: "No inline styles: use token-based Tailwind classes (CSP style-src 'self').",
        },
        {
          selector: "JSXAttribute[name.name='dangerouslySetInnerHTML']",
          message: "No raw HTML in the app frame. Mail HTML goes through the sanitising pipeline only.",
        },
      ],
    },
  },

  // Tests may use the Tauri mocks and simulate framework behaviour.
  {
    files: ["tests/**/*.{ts,tsx}"],
    rules: {
      "@typescript-eslint/unbound-method": "off",
    },
  },
);
