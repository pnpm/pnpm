---
"pacquet": patch
---

`pnpm peers check`, `why`, `list`, `ll`, `licenses`, `audit`, `sbom`, `fetch`, `patch`, `patch-commit`, `patch-remove`, `approve-builds` and `runtime` now honor settings applied by an `updateConfig` hook. Hook-provided catalogs now resolve the `catalog:` peer dependencies of linked workspace packages in `pnpm peers check`. `pnpm fetch` no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` when a hook supplies the catalog recorded in the lockfile [pnpm/pnpm#15047](https://github.com/pnpm/pnpm/issues/15047).
