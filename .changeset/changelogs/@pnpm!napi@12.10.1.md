## 12.10.1

### Patch Changes

- `rebuild()` no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` on a lockfile that records a `pnpmfileChecksum` or was resolved with other settings. Of the settings the lockfile records, `rebuild()` now checks only `patchedDependencies`.
