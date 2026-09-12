---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` after moving a dependency between `dependencies`, `devDependencies`, and `optionalDependencies` now updates the lockfile in place instead of re-resolving the whole dependency graph [#13696](https://github.com/pnpm/pnpm/issues/13696).
