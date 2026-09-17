---
"@pnpm/global.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update --global` now reports `Already up to date` when the global package graph has not changed [pnpm/pnpm#12002](https://github.com/pnpm/pnpm/issues/12002).
