---
"pacquet": patch
---

`pnpm dedupe` no longer changes the lockfile on every run when a dependency's range matches both a direct dependency and a newer `npm:` alias of it [pnpm/pnpm#15588](https://github.com/pnpm/pnpm/issues/15588).
