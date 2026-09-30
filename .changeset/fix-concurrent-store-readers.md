---
"@pnpm/store.index": major
"@pnpm/store.connection-manager": patch
"@pnpm/building.after-install": patch
"@pnpm/worker": patch
"pnpm": patch
---

`ReadOnlyStoreIndex` now observes concurrent writes to a shared store safely. Concurrent writes could previously cause "database disk image is malformed" errors or stale reads. Callers reading a finalized store on a read-only filesystem must use `ImmutableStoreIndex`. The `frozenStore` setting continues to use immutable access.
