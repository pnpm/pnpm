## 1103.0.15

### Patch Changes

- Fixed `pnpm store status` reporting untouched packages as modified when the global virtual store is enabled. If the installed global-store layout cannot be verified, the command reports `ERR_PNPM_STORE_STATUS_UNVERIFIABLE` and asks for a reinstall. [pnpm/pnpm#17631](https://github.com/pnpm/pnpm/issues/17631).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.41
  - @pnpm/deps.graph-builder@1101.1.9
  - @pnpm/deps.graph-hasher@1100.3.11
  - @pnpm/exec.lifecycle@1100.1.28
  - @pnpm/installing.context@1101.0.15
  - @pnpm/installing.deps-restorer@1103.2.9
  - @pnpm/lockfile.settings-checker@1100.2.17
  - @pnpm/lockfile.utils@1102.1.10
  - @pnpm/store.connection-manager@1101.3.8
  - @pnpm/workspace.task-scheduler@1100.0.7
