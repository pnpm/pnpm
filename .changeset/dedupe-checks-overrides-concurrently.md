---
"pacquet": patch
---

`pnpm dedupe` and `pnpm install` now check every convergence override at once after resolution. The check previously waited for each override's registry lookups to finish before starting the next one, so projects with many convergence overrides and a slow registry took much longer than on pnpm 11 [#15175](https://github.com/pnpm/pnpm/issues/15175).
