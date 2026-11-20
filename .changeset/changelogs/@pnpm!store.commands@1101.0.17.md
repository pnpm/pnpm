## 1101.0.17

### Patch Changes

- `pnpm cat-file` now reports `ERR_PNPM_INVALID_HASH` when given an empty, malformed, or truncated digest such as `sha512-`.

- Updated dependencies:
  - @pnpm/building.pkg-requires-build@1100.0.23
  - @pnpm/building.policy@1100.1.11
  - @pnpm/cli.utils@1101.0.36
  - @pnpm/config.normalize-registries@1101.0.6
  - @pnpm/config.reader@1102.3.10
  - @pnpm/deps.graph-builder@1101.1.11
  - @pnpm/deps.path@1101.0.10
  - @pnpm/fs.symlink-dependency@1100.0.27
  - @pnpm/global.packages@1101.1.12
  - @pnpm/installing.client@1100.3.22
  - @pnpm/installing.context@1101.0.17
  - @pnpm/lockfile.types@1100.1.8
  - @pnpm/lockfile.utils@1102.1.12
  - @pnpm/store.cafs@1100.3.11
  - @pnpm/store.connection-manager@1101.3.10
  - @pnpm/store.controller-types@1101.3.7
  - @pnpm/types@1102.1.4
