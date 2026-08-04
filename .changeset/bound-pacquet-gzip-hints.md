---
"pacquet": patch
---

Limit registry-provided gzip preallocation hints to 64 MiB so oversized `dist.unpackedSize` values cannot trigger excessive eager allocation.
