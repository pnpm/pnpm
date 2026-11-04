---
"pacquet": patch
---

`pnpm dedupe --check` leaves `pnpm-lock.yaml` untouched while it looks for deduplication changes. Previously it rewrote the lockfile and then restored it. If `configDependencies` changed since the last install, the check keeps the lockfile they sync, as `pnpm install` does [pnpm/pnpm#17628](https://github.com/pnpm/pnpm/issues/17628).
