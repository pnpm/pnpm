---
"pacquet": patch
---

`pnpm add` and other installs that re-resolve dependencies now keep the locked `devEngines.runtime` version while it still satisfies the declared range [#16764](https://github.com/pnpm/pnpm/issues/16764).
