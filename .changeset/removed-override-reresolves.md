---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

Removing or changing an entry in `overrides` now re-resolves the packages it targeted. A version the override had locked is no longer kept just because the declared range still accepts it [#4587](https://github.com/pnpm/pnpm/issues/4587).
