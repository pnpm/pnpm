---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now re-resolves a workspace project's auto-installed peer dependency when its locked version no longer matches another workspace project's specifier for that package. Both projects then resolve the same version [#11800](https://github.com/pnpm/pnpm/issues/11800).
