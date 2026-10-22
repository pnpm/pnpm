---
"@pnpm/pnpr": patch
---

Packument responses now emit `ETag`, `Vary: Accept`, and `Cache-Control` headers. Conditional requests sending `If-None-Match` or `If-Modified-Since` now return `304 Not Modified` when unchanged.
