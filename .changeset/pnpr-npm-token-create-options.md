---
"@pnpm/pnpr": patch
---

`POST /-/npm/v1/tokens` now honors `packages_and_scopes_permission` and interprets integer `expires` values in days, matching `npm token create`. Unsupported package, scope, organization, and stage-only token restrictions now return `400 Bad Request`.
