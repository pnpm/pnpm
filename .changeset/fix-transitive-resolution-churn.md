---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Avoid lockfile churn for unrelated transitive dependencies when adding a dependency [https://github.com/pnpm/pnpm/issues/11456](https://github.com/pnpm/pnpm/issues/11456).
