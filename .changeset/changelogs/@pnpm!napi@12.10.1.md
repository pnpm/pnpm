## 12.10.1

### Patch Changes

- `rebuild()` failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` on a lockfile that records a `pnpmfileChecksum`, or that was resolved with other settings. Of the settings the lockfile records, `rebuild()` now checks only `patchedDependencies` against the options, as `pnpm rebuild` did in pnpm v11.
