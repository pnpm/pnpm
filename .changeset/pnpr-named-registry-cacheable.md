---
"@pnpm/pnpr": patch
---

Responses for public packages read through a named registry, such as `/npm/~npmjs/<package>`, are now cacheable by shared HTTP caches. Packages that need authentication still get `Cache-Control: private, no-store`. PyPI project pages now send `Vary: Accept`.
