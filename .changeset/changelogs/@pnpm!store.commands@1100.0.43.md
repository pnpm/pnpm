## 1100.0.43

### Patch Changes

- `pnpm store prune` no longer deletes the lockfile verification log. The log records which lockfile passed which supply-chain policies, so it stays valid across a prune of the store; keeping it lets the next install skip re-verifying an unchanged lockfile.

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.23
  - @pnpm/config.reader@1101.17.0
  - @pnpm/crypto.integrity@1100.0.4
  - @pnpm/error@1100.1.2
  - @pnpm/global.packages@1100.0.18
  - @pnpm/installing.client@1100.3.4
  - @pnpm/installing.context@1100.1.2
  - @pnpm/lockfile.utils@1101.1.0
  - @pnpm/store.cafs@1100.1.19
  - @pnpm/store.connection-manager@1100.3.17
  - @pnpm/store.controller-types@1101.1.1
  - @pnpm/store.index@1100.2.4
  - @pnpm/store.path@1100.0.5
