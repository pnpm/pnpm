---
"@pnpm/pnpr": patch
---

pnpr now honors `Cache-Control: no-cache` on upstream package metadata requests. A failed refresh for an unlisted version returns the upstream error and can be retried immediately.
