## 1101.1.9

### Patch Changes

- Fixed `pnpm store status` reporting untouched packages as modified when the global virtual store is enabled. If the installed global-store layout cannot be verified, the command reports `ERR_PNPM_STORE_STATUS_UNVERIFIABLE` and asks for a reinstall. [pnpm/pnpm#17631](https://github.com/pnpm/pnpm/issues/17631).

- Updated dependencies:
  - @pnpm/deps.graph-hasher@1100.3.11
  - @pnpm/lockfile.fs@1100.2.17
  - @pnpm/lockfile.utils@1102.1.10
