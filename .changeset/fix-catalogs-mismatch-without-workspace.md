---
"@pnpm/lockfile.settings-checker": patch
"pnpm": patch
---

Fixed `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` error when the lockfile contains a `catalogs` section but no `pnpm-workspace.yaml` exists [#10551](https://github.com/pnpm/pnpm/issues/10551).
