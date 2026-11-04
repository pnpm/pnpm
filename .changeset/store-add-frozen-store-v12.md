---
"pacquet": patch
---

`pnpm store add` now fails with `ERR_PNPM_FROZEN_STORE_WRITE` when `frozenStore` is enabled. It used to write package files into the read-only store without recording them, so no install could use them.
