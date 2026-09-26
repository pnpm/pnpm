---
"@pnpm/network.fetch": patch
"pnpm": patch
"pacquet": patch
---

A fetch timeout while other downloads are still running now lowers network concurrency to one connection. Retries of that request, and later downloads, use the lower concurrency [pnpm/pnpm#12791](https://github.com/pnpm/pnpm/issues/12791).
