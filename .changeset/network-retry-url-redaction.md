---
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/network.fetch": patch
"pnpm": patch
---

Network and archive retry logs hide credentials and signed query parameters in request URLs.
