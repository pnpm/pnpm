---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm update` and `pnpm audit --fix=update` now preserve dependency specifications supplied by `packageExtensions`, `readPackage` hooks, or overrides instead of writing them to the project manifest [#14928](https://github.com/pnpm/pnpm/issues/14928).
