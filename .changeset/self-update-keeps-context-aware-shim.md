---
"pacquet": patch
---

`pnpm self-update` now keeps a context-aware global `pnpm` shim and points it at the new version. Before, it left the shim running the old version and wrote direct shims beside or over it.
