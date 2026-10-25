---
"pnpm": patch
"pacquet": patch
---

`pnpm ci --lockfile` and `pnpm ci --config.lockfile=true` no longer delete `pnpm-lock.yaml`. `pnpm ci` now always keeps the lockfile and runs a frozen install from it.
