---
"@pnpm/pnpr": patch
---

Deleting an OCI manifest by digest now invalidates saved batch publish outcomes that included it, while tag-only deletions leave batch replay intact and deleted OCI manifests can still be republished.
