---
"pacquet": patch
---

`pnpm dedupe` now reaches a stable lockfile when a package's peer suffix is long enough to be hashed. Before, each run could switch that package's key between the hashed and the spelled-out suffix, so `pnpm dedupe --check` always failed [#16331](https://github.com/pnpm/pnpm/issues/16331).
