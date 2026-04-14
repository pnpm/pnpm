---
"@pnpm/deps.inspection.commands": minor
"@pnpm/network.web-auth": major
"@pnpm/auth.commands": patch
"@pnpm/releasing.commands": patch
"pnpm": minor
---

Added the `pnpm docs` command and its alias `pnpm home`. This command opens the package documentation or homepage in the browser. When the package has no valid homepage, it falls back to `https://npmx.dev/package/<name>`.

Internally, `@pnpm/network.web-auth`'s `promptBrowserOpen` now uses the [`open`](https://www.npmjs.com/package/open) package instead of spawning platform-specific commands. The `execFile` field and `PromptBrowserOpenExecFile` / `PromptBrowserOpenProcess` type exports have been removed from `PromptBrowserOpenContext`.
