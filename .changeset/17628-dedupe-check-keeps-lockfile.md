---
"pacquet": patch
---

`pnpm dedupe --check` no longer writes `pnpm-lock.yaml` to find deduplication changes. Tools that watch the lockfile no longer see it change during a check [pnpm/pnpm#17628](https://github.com/pnpm/pnpm/issues/17628).
