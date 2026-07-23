import js from "@eslint/js";
import ts from "typescript-eslint";
import svelte from "eslint-plugin-svelte";
import globals from "globals";

export default [
  js.configs.recommended,
  ...ts.configs.recommended,
  ...svelte.configs["flat/recommended"],
  {
    languageOptions: {
      globals: { ...globals.browser, ...globals.node },
    },
    rules: {
      // Ban the swallow (WI #547).
      //
      // `.catch(() => [])` and friends turn a failed request into a successful
      // empty result. The review found this across the app, worst on link-up,
      // where four collections did it at once — an API outage rendered a page
      // that confidently said there was nothing to link. Nothing was broken on
      // screen; the page simply asserted something false.
      //
      // This is a lint rule rather than a review habit because it is invisible
      // by construction: the swallow looks like defensive code, and the bug it
      // causes looks like a normal empty state.
      //
      // If you genuinely want a failure to be non-fatal, say so out loud —
      // `attempt()` from $lib/toast.svelte reports it and returns undefined.
      "no-restricted-syntax": [
        "error",
        {
          selector:
            "CallExpression[callee.property.name='catch'] > ArrowFunctionExpression[body.type=/ArrayExpression|ObjectExpression/]",
          message:
            "Don't swallow a failed request into an empty value — a failed load must be distinguishable from an empty one. Use attempt() from $lib/toast.svelte, or catch and set an error state rendered by <ErrorNotice>.",
        },
        {
          selector:
            "CallExpression[callee.property.name='catch'] > ArrowFunctionExpression[body.type='BlockStatement'][body.body.length=0]",
          message:
            "An empty catch hides a failure entirely. Use attempt() from $lib/toast.svelte, or set an error state rendered by <ErrorNotice>.",
        },
      ],
    },
  },
  {
    files: ["**/*.svelte", "**/*.svelte.ts"],
    languageOptions: {
      parserOptions: { parser: ts.parser },
    },
    rules: {
      "prefer-const": "off",
    },
  },
  {
    ignores: [
      "build/",
      ".svelte-kit/",
      "dist/",
      "node_modules/",
      "playwright-report/",
      "test-results/",
    ],
  },
];
