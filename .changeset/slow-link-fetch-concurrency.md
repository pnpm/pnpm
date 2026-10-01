---
"@pnpm/network.fetch": patch
"pnpm": patch
"pacquet": patch
---

A fetch timeout while other downloads from the same host are still running now lowers concurrency for that host to one connection. Retries of that request, and later downloads from that host, use the lower concurrency. Other hosts keep the configured concurrency [pnpm/pnpm#12791](https://github.com/pnpm/pnpm/issues/12791).
