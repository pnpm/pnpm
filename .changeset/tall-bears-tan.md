---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

`bundledDependencies` should never be added to the lockfile with `false` as the value [#7576](https://github.com/pnpm/pnpm/issues/7576).
