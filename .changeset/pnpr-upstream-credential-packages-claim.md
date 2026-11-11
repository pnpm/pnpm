---
"@pnpm/pnpr": patch
---

A resolve request no longer sends a private upstream's credential when it fetches a name outside that upstream's `packages:` map. This applies even when the request names the upstream's URL as its registry.
