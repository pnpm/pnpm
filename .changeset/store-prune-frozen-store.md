---
"pacquet": patch
---

`pnpm store prune` now fails with `ERR_PNPM_FROZEN_STORE_WRITE` when `frozenStore` is enabled. It used to remove files from the store.
