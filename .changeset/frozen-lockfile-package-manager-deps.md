---
"@pnpm/installing.env-installer": patch
"pacquet": patch
"pnpm": patch
---

A frozen install no longer rewrites the `packageManagerDependencies` block of `pnpm-lock.yaml`. When the pnpm version pinned by `devEngines.packageManager` (or by `packageManager`) is missing from the lockfile or no longer matches it, `--frozen-lockfile` now fails with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` instead of resolving the version and saving it, so a manifest whose pin was bumped without regenerating the lockfile can no longer pass CI [#14009](https://github.com/pnpm/pnpm/issues/14009).
