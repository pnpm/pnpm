---
"@pnpm/plugin-commands-installation": patch
"@pnpm/plugin-commands-deploy": patch
"pnpm": patch
---

Always set `dedupe-peer-dependents` to `false`, when running installation during deploy [#6858](https://github.com/pnpm/pnpm/issues/6858).
