---
"pacquet": patch
---

`pnpm dedupe` no longer switches a dependency back and forth between two versions on every run when its range matches both a direct dependency and a newer `npm:` alias of that dependency. It keeps the version of the direct dependency, and `pnpm dedupe --check` passes after a dedupe [pnpm/pnpm#15588](https://github.com/pnpm/pnpm/issues/15588).
