## 1103.0.8

### Patch Changes

- `ReadOnlyStoreIndex` now observes concurrent writes to a shared store safely. Concurrent writes could previously cause "database disk image is malformed" errors or stale reads. Callers reading a finalized store on a read-only filesystem must use `ImmutableStoreIndex`. The `frozenStore` setting continues to use immutable access.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.35
  - @pnpm/config.reader@1102.3.2
  - @pnpm/exec.lifecycle@1100.1.21
  - @pnpm/installing.context@1101.0.8
  - @pnpm/installing.deps-restorer@1103.2.2
  - @pnpm/lockfile.settings-checker@1100.2.10
  - @pnpm/store.connection-manager@1101.3.1
  - @pnpm/store.index@1101.0.0
