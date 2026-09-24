---
"pacquet": patch
---

`pnpm dedupe` now produces a stable lockfile when a dependency's range matches both a direct dependency and an `npm:` alias of the same package. The dependency resolves to the version of the direct dependency. Repeated runs previously alternated between two lockfiles [#15588](https://github.com/pnpm/pnpm/issues/15588).
