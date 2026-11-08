---
"@pnpm/pnpr": patch
---

Deleting an OCI manifest by digest now invalidates the saved batch publish outcomes that include it. Deleting an OCI tag leaves batch replay intact. A deleted OCI manifest can be published again.
