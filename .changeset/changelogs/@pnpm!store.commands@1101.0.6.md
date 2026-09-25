## 1101.0.6

### Patch Changes

- `pnpm store prune` now leaves a `dlx` cache root that is a symlink or Windows junction untouched. Cleanup no longer removes directories through that link.

- `pnpm store status` no longer falsely reports packages with build or postinstall scripts as modified in the store. Packages requiring builds have their integrity verified against recorded side effects, or are recognized as built packages.

- Updated dependencies:
  - @pnpm/building.pkg-requires-build@1100.0.18
  - @pnpm/cli.utils@1101.0.29
  - @pnpm/config.normalize-registries@1101.0.2
  - @pnpm/config.reader@1102.3.0
  - @pnpm/crypto.integrity@1100.0.7
  - @pnpm/deps.path@1101.0.4
  - @pnpm/error@1100.2.0
  - @pnpm/fs.graceful-fs@1100.2.3
  - @pnpm/global.packages@1101.1.4
  - @pnpm/installing.client@1100.3.11
  - @pnpm/installing.context@1101.0.6
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/store.cafs@1100.3.4
  - @pnpm/store.connection-manager@1101.2.0
  - @pnpm/store.controller-types@1101.3.1
  - @pnpm/store.index@1100.3.2
  - @pnpm/store.path@1100.0.8
  - @pnpm/types@1102.1.1
