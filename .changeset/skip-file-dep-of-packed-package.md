---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` no longer fails when a package declares a `file:` dependency that points inside that package. The dependency is skipped [#9141](https://github.com/pnpm/pnpm/issues/9141).
