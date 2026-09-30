---
"@pnpm/store.index": patch
"pnpm": patch
---

Tools reading a shared store can use `ConcurrentReadOnlyStoreIndex` to observe concurrent writes safely. Using `ReadOnlyStoreIndex` for a changing store could report "database disk image is malformed" or return stale entries. `ReadOnlyStoreIndex` retains its immutable behavior for finalized stores and `frozenStore`.
