---
"pacquet": patch
---

A frozen install whose recorded settings no longer match the configuration — `overrides`, `catalogs`, `patchedDependencies`, and the rest — now fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` naming the one setting that changed, instead of `ERR_PNPM_OUTDATED_LOCKFILE` with the whole map dumped [#13322](https://github.com/pnpm/pnpm/issues/13322).
