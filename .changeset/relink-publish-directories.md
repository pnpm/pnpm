---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/lockfile.fs": patch
"@pnpm/lockfile.types": patch
"@pnpm/lockfile.verification": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now relinks workspace packages when `publishConfig.linkDirectory` changes. Frozen installs report an outdated lockfile until it is regenerated [pnpm/pnpm#14488](https://github.com/pnpm/pnpm/issues/14488).
