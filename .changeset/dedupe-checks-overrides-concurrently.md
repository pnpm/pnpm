---
"pacquet": patch
---

Sped up `pnpm dedupe` and `pnpm install` in projects with many convergence overrides. The check for stale convergence overrides now runs its registry lookups for every override at once, so on a slow registry its cost no longer grows with the number of overrides [#15175](https://github.com/pnpm/pnpm/issues/15175).
