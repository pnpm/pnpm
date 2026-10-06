---
"pacquet": patch
---

`pnpm deploy` with a shared lockfile no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` for `settings.dedupeInjectedDeps` or `settings.dedupePeerDependents` when `lockfile.includeResolutionSettings` is enabled. The deployed lockfile now records both settings as `false`, the values the deploy installs with.
