---
"@pnpm/pnpr": patch
---

pnpr aborts proxied blob downloads when their contents fail integrity verification. Invalid blobs remain excluded from the cache.
