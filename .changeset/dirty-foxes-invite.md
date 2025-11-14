---
"@pnpm/tools.plugin-commands-self-updater": patch
"pnpm": patch
---

`pnpm self-update` should always install the non-executable pnpm package (pnpm in the registry) and never the `@pnpm/exe` package, when installing v11 or newer. We currently cannot ship `@pnpm/exe` as `pkg` doesn't work with ESM [#10190](https://github.com/pnpm/pnpm/pull/10190).
