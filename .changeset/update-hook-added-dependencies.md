---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm update` and `pnpm audit --fix=update` now leave dependency specifications and resolved versions unchanged when they are owned by `packageExtensions`, `readPackage` hooks, or overrides [#14928](https://github.com/pnpm/pnpm/issues/14928).
