---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm self-update` run in a project that pins pnpm through `packageManager` or `devEngines.packageManager` now installs the resolved version globally as well as updating the pin. It updated only the pin before, so the active pnpm stayed on the old version [#14747](https://github.com/pnpm/pnpm/issues/14747).
