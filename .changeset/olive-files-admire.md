---
"@pnpm/resolve-dependencies": patch
"@pnpm/lockfile-types": patch
"@pnpm/types": patch
"pnpm": patch
---

Added support for boolean values in 'bundleDependencies' package.json fields when installing a dependency. Fix to properly handle 'bundledDependencies' alias [#7411](https://github.com/pnpm/pnpm/issues/7411).
