---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/releasing.commands": patch
"@pnpm/lockfile.fs": patch
"@pnpm/lockfile.utils": patch
"@pnpm/store.controller-types": patch
"@pnpm/resolving.resolver-base": patch
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy --offline --trust-lockfile` now reuses dependencies pinned in the source lockfile from a populated store when registry metadata is unavailable. This works with per-project lockfiles, `--legacy`, and npm aliases [pnpm/pnpm#17888](https://github.com/pnpm/pnpm/issues/17888).
