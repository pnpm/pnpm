## 1100.4.6

### Patch Changes

- `ReadOnlyStoreIndex` now observes concurrent writes to a shared store safely. Concurrent writes could previously cause "database disk image is malformed" errors or stale reads. Callers reading a finalized store on a read-only filesystem must use `ImmutableStoreIndex`. The `frozenStore` setting continues to use immutable access.

- Updated dependencies:
  - @pnpm/store.index@1101.0.0
