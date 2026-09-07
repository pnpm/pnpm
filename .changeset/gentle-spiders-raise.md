---
"@pnpm/deps.status": patch
"@pnpm/installing.context": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/lockfile.fs": patch
"@pnpm/lockfile.utils": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` now repairs a `pnpm-lock.yaml` whose `(patch_hash=<hash>)` dependency paths disagree with its `patchedDependencies` map. Such a lockfile was previously accepted as up to date, so an install could keep applying an outdated patch. `pnpm install --frozen-lockfile` now reports `ERR_PNPM_INCONSISTENT_PATCH_HASH` for it, or `ERR_PNPM_UNCHECKABLE_PATCH_HASH` when the lockfile is missing what checking needs [#15336](https://github.com/pnpm/pnpm/pull/15336).
