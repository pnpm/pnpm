---
"@pnpm/resolving.default-resolver": minor
"@pnpm/resolving.npm-resolver": minor
"pacquet": patch
"pnpm": patch
---

Lockfile verification now honors offline mode by using cached registry metadata instead of reaching the registry. When the required metadata is not available locally, verification reports the same `ERR_PNPM_NO_OFFLINE_META` condition used by offline resolution.
