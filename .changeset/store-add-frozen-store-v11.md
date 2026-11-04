---
"@pnpm/store.commands": patch
"pnpm": patch
---

`pnpm store add` now fails right away with `ERR_PNPM_FROZEN_STORE_WRITE` when `frozenStore` is enabled. It used to retry each download for minutes before failing, or fail with a SQLite error on an empty store.
