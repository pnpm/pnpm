---
"@pnpm/deps.status": patch
"@pnpm/installing.context": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/lockfile.fs": patch
"@pnpm/patching.config": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` now repairs a `pnpm-lock.yaml` whose `(patch_hash=<hash>)` dependency paths disagree with its `patchedDependencies` map, including paths that lack the hash their patch calls for. pnpm previously accepted such a lockfile as up to date and kept the old patched files. `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_INCONSISTENT_PATCH_HASH` for such a lockfile, or with `ERR_PNPM_UNCHECKABLE_PATCH_HASH` when the lockfile lacks the package version or patch entry that the check needs [#15336](https://github.com/pnpm/pnpm/pull/15336).
