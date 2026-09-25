## 1102.0.3

### Patch Changes

- `pnpm update --global` now skips a global package installed from a `file:` path that no longer exists, prints a warning, and updates the remaining global packages. Previously the whole update failed with `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND` [#12533](https://github.com/pnpm/pnpm/issues/12533).

- `pnpm update -g` no longer asks more than once for approval of the same immature `name@version` when `minimumReleaseAgeStrict` is enabled [#15091](https://github.com/pnpm/pnpm/issues/15091).

- Fixed `pnpm install` failing with `EEXIST` when a concurrent install cleared the file or directory that was occupying a symlink path. On Windows, a symlink another process is still holding is no longer moved aside and recreated.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.33
  - @pnpm/bins.remover@1100.0.25
  - @pnpm/bins.resolver@1100.0.17
  - @pnpm/cli.utils@1101.0.29
  - @pnpm/config.reader@1102.3.0
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/deps.inspection.list@1101.0.6
  - @pnpm/error@1100.2.0
  - @pnpm/global.packages@1101.1.4
  - @pnpm/installing.deps-installer@1104.2.0
  - @pnpm/installing.modules-yaml@1101.0.4
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/pkg-manifest.utils@1100.4.6
  - @pnpm/resolving.local-resolver@1101.2.3
  - @pnpm/store.connection-manager@1101.2.0
  - @pnpm/types@1102.1.1
