---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Adding a dependency no longer moves an unrelated transitive dependency to another version that is already in the lockfile [#11456](https://github.com/pnpm/pnpm/issues/11456).
