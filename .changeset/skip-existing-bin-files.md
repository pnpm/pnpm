---
"@pnpm/bins.linker": patch
"pnpm": patch
---

Skip linking bins that already reference the correct target. This avoids redundant I/O during repeated installs and prevents permission errors when the store is read-only (e.g. Docker layer caching, CI prewarm, NFS). Bins that reference a stale or incorrect target are still rewritten.
