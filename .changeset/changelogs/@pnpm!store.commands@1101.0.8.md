## 1101.0.8

### Patch Changes

- `pnpm store prune` now removes the packages that only expired `pnpm dlx` cache entries used, as long as the store still has another registered project. They were left in the store until the next `pnpm store prune` [#16383](https://github.com/pnpm/pnpm/discussions/16383).

- Updated dependencies:
  - @pnpm/config.reader@1102.3.2
  - @pnpm/global.packages@1101.1.6
  - @pnpm/installing.client@1100.3.13
  - @pnpm/installing.context@1101.0.8
  - @pnpm/store.connection-manager@1101.3.1
  - @pnpm/store.index@1101.0.0
