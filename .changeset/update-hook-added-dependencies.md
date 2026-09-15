---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm audit --fix=update` no longer writes dependencies added by `packageExtensions` or `readPackage` hooks to `package.json` [#14928](https://github.com/pnpm/pnpm/issues/14928).
