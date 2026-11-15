---
"pnpm": patch
"pacquet": patch
---

`pnpm doctor` now checks write access for the cache, store, and global bin directories before they have been created. Previously, `pnpm doctor` reported `no write access` on fresh environments.

`pnpm doctor` now probes whether reflinks or hardlinks work from the store into the project's link target and warns when neither does.
