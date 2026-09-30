---
"@pnpm/pnpr.client": patch
"pnpm": patch
---

Resolving through a pnpr server no longer fails with `ERR_PNPM_PNPR_PUBLISH_DIRECTORY_MISMATCH` when a workspace project lives in a directory named `constructor`.
