---
"pacquet": patch
---

`pnpm dedupe --check` no longer rewrites and restores `pnpm-lock.yaml`. If `configDependencies` changed since the last install, the check now leaves the synced lockfile in place, as `pnpm install` does [pnpm/pnpm#17628](https://github.com/pnpm/pnpm/issues/17628).
