---
"@pnpm/store.index": patch
"@pnpm/store.connection-manager": patch
"@pnpm/building.after-install": patch
"@pnpm/worker": patch
"pnpm": patch
---

Fixed `ReadOnlyStoreIndex` reporting "database disk image is malformed" or returning stale entries when another process writes to the shared store. Callers reading a finalized store on a read-only filesystem should use `ImmutableStoreIndex`. The `frozenStore` setting continues to use immutable access.
