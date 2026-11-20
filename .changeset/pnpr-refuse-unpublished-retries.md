---
"@pnpm/pnpr": patch
---

Retrying an identical publish of an unpublished version now returns `403`. Batch publish retries also fail if any published member has been unpublished.
