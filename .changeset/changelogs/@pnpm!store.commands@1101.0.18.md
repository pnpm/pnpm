## 1101.0.18

### Patch Changes

- `pnpm cat-file` can read executable files from the store. `pnpm cache list`, `view`, and `delete` recognize mixed-case package names.

  `pnpm clean` removes custom virtual stores with hidden directory names. In Plug'n'Play projects it also removes the generated loader on a full clean.

- `pnpm store prune` now fails with `ERR_PNPM_FROZEN_STORE_WRITE` when `frozenStore` is enabled. It used to remove files from the store.

- Updated dependencies:
  - @pnpm/building.pkg-requires-build@1100.0.24
  - @pnpm/building.policy@1100.1.12
  - @pnpm/cli.utils@1101.0.37
  - @pnpm/config.normalize-registries@1101.0.7
  - @pnpm/config.reader@1102.4.0
  - @pnpm/crypto.integrity@1100.0.10
  - @pnpm/deps.graph-builder@1101.1.12
  - @pnpm/deps.path@1101.0.11
  - @pnpm/error@1100.2.3
  - @pnpm/fs.graceful-fs@1100.2.7
  - @pnpm/fs.symlink-dependency@1100.0.28
  - @pnpm/global.packages@1101.1.13
  - @pnpm/installing.client@1100.3.23
  - @pnpm/installing.context@1101.0.18
  - @pnpm/lockfile.types@1100.1.9
  - @pnpm/lockfile.utils@1102.1.13
  - @pnpm/store.cafs@1100.3.12
  - @pnpm/store.connection-manager@1101.3.11
  - @pnpm/store.controller-types@1101.3.8
  - @pnpm/store.index@1101.0.5
  - @pnpm/store.path@1100.1.4
  - @pnpm/types@1102.1.5
