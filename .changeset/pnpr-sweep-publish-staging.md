---
"@pnpm/pnpr": patch
---

`pnpr` now reclaims abandoned publish staging files once they are older than `requestTimeout` at startup, periodically while serving, and during `pnpr cache-gc`. Cancelled or timed-out publishes also remove their staging files immediately.
