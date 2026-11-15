---
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm doctor` reporting `no write access` on fresh environments before the cache, store, or global bin directories exist.

`pnpm doctor` now probes whether reflinks or hardlinks work from the store into the project's link target and warns when neither does.
