---
"@pnpm/network.fetch": minor
"@pnpm/fetching.tarball-fetcher": minor
"pnpm": minor
---

Improved HTTP performance with HTTP/2 multiplexing, connection pipelining, Happy Eyeballs (dual-stack), and better keep-alive settings. Tarball downloads with known size now pre-allocate memory to avoid double-copy overhead.
