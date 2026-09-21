---
"pacquet": patch
---

`pnpm peers check`, `why`, `list`, `ll`, `licenses`, `audit`, `sbom`, `fetch`, `patch`, `patch-commit`, `patch-remove`, `approve-builds` and `runtime` now honor settings applied by an `updateConfig` hook. Hook-provided catalogs now resolve the `catalog:` peer dependencies of linked workspace packages in `pnpm peers check` [pnpm/pnpm#15047](https://github.com/pnpm/pnpm/issues/15047). `pnpm fetch` previously failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` when a hook supplied the catalog recorded in the lockfile. It now honors hook-provided catalogs when checking the lockfile config [pnpm/pnpm#15049](https://github.com/pnpm/pnpm/pull/15049).
