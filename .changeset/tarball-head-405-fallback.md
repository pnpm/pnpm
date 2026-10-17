---
"@pnpm/resolving.tarball-resolver": patch
"pnpm": patch
"pacquet": patch
---

Tarball URLs that reject HEAD requests with 405 Method Not Allowed now fall back to a GET request during resolution.
