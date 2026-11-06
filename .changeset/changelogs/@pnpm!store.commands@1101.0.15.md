## 1101.0.15

### Patch Changes

- `pnpm store add` now fails right away with `ERR_PNPM_FROZEN_STORE_WRITE` when `frozenStore` is enabled. It used to retry each download for minutes before failing, or fail with a SQLite error on an empty store.

- Fixed `pnpm store status` reporting untouched packages as modified when the global virtual store is enabled. If the installed global-store layout cannot be verified, the command reports `ERR_PNPM_STORE_STATUS_UNVERIFIABLE` and asks for a reinstall. [pnpm/pnpm#17631](https://github.com/pnpm/pnpm/issues/17631).

- Updated dependencies:
  - @pnpm/deps.graph-builder@1101.1.9
  - @pnpm/global.packages@1101.1.10
  - @pnpm/installing.client@1100.3.20
  - @pnpm/installing.context@1101.0.15
  - @pnpm/lockfile.utils@1102.1.10
  - @pnpm/store.connection-manager@1101.3.8
