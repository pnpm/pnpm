---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm self-update` run in a project that pins pnpm through `packageManager` or `devEngines.packageManager` now also updates the global pnpm, as it does outside a project. It updated only the pin before, so the active pnpm stayed on the old version [#14747](https://github.com/pnpm/pnpm/issues/14747).
