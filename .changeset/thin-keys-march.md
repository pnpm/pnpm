---
"@pnpm/plugin-commands-publishing": patch
"pnpm": patch
---

`prepublishOnly` and `prepublish` should not be executed on `pnpm pack` [#2941](https://github.com/pnpm/pnpm/issues/2941).
