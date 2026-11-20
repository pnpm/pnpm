---
"@pnpm/pnpr": patch
"@pnpm/pnpr.client": patch
"pnpm": patch
"pacquet": patch
---

pnpr now advertises capabilities through `pnpr.protocols` at the server root and npm registry bases. pnpm reads this capability map for remote resolution and shared build caches. The HTTP protocol path remains `/v0/`.
