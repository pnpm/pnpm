---
"@pnpm/hooks.pnpmfile": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/lockfile.fs": patch
"@pnpm/lockfile.merger": patch
"@pnpm/lockfile.pruner": patch
"@pnpm/lockfile.types": patch
"@pnpm/napi": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` now applies changes to or removal of a global `readPackage` hook when an existing lockfile is present [pnpm/pnpm#15136](https://github.com/pnpm/pnpm/issues/15136).
