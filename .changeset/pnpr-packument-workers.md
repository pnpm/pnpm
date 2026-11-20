---
"@pnpm/pnpr": patch
---

Large package metadata requests no longer block unrelated registry requests. Repeated requests reuse rendered metadata while the package content is unchanged.
