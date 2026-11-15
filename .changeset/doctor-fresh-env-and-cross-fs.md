---
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm doctor` reporting `no write access` on fresh environments before the cache, store, or global bin directories exist.

`pnpm doctor` now probes reflink and hardlink support between the store and the project and warns when they are on different filesystems.
