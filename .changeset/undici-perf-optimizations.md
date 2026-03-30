---
"@pnpm/network.fetch": minor
"@pnpm/fetching.tarball-fetcher": minor
"pnpm": minor
---

Improved HTTP performance with Happy Eyeballs (dual-stack), better keep-alive settings, and an optimized global dispatcher. Tarball downloads with known size now pre-allocate memory to avoid double-copy overhead.
