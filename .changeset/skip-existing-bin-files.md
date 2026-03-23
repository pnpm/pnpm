---
"@pnpm/bins.linker": patch
"pnpm": patch
---

Skip linking bins that already exist on disk. This avoids redundant I/O during repeated installs and prevents permission errors when the store is read-only (e.g. Docker layer caching, CI prewarm, NFS).
