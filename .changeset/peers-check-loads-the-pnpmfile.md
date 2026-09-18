---
"pacquet": patch
---

`pnpm peers check`, `why`, `list`, `ll`, `licenses`, `audit`, `sbom`, `fetch`, `patch` and `runtime` now load the pnpmfile, so the settings an `updateConfig` hook changes reach them. Catalogs a hook adds now resolve the `catalog:` peer dependencies of linked workspace packages in `pnpm peers check` [pnpm/pnpm#15047](https://github.com/pnpm/pnpm/issues/15047), and `pnpm fetch` no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` when a hook supplied the catalog the lockfile records.
