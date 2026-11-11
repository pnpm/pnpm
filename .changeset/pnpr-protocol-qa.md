---
"@pnpm/pnpr": patch
---

Cargo publishes to an unclaimed package now return 403 and name the `packages:` setting in the error. Unsupported npm visibility and collaborator endpoints return an explanatory error.
