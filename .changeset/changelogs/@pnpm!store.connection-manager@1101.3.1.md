## 1101.3.1

### Patch Changes

- `ReadOnlyStoreIndex` now observes concurrent writes to a shared store safely. Concurrent writes could previously cause "database disk image is malformed" errors or stale reads. Callers reading a finalized store on a read-only filesystem must use `ImmutableStoreIndex`. The `frozenStore` setting continues to use immutable access.

- Updated dependencies:
  - @pnpm/config.reader@1102.3.2
  - @pnpm/installing.client@1100.3.13
  - @pnpm/store.controller@1102.2.2
  - @pnpm/store.index@1101.0.0
