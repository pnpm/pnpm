---
"@pnpm/pnpr": patch
---

`cache.maxSize`, `cache.maxAge`, and `pnpr cache-gc` now account for and evict across the whole cache directory, including `resolver-store`, `resolver-cache`, and `lockfile-verdicts.sqlite`. pnpr refuses to start without `secret` or `secrets.cache` when the cache persists across restarts, is shared with another process, or uses shared storage or database backends. The default OSV database path moved to `<storage>/.osv/npm/all.zip`.
