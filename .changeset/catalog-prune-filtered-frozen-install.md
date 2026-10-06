---
"pacquet": patch
---

A frozen install with `catalogPrune` no longer removes catalog entries that `pnpm-lock.yaml` still records. Before, `pnpm install --frozen-lockfile --filter` failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` when some workspace projects were missing from disk [#16638](https://github.com/pnpm/pnpm/issues/16638).
