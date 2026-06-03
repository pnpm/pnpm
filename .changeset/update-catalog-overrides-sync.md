---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm update` now keeps lockfile `overrides` that resolve through a catalog in sync with the catalog. Previously, when an override referenced a catalog (e.g. `overrides: { foo: 'catalog:' }`) and `pnpm update` bumped that catalog entry, the lockfile's `catalogs` advanced while the resolved `overrides` kept the old version. The resulting lockfile was internally inconsistent, so a later `pnpm install --frozen-lockfile` failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH`.
