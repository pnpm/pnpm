---
"@pnpm/pnpr": minor
---

pnpr now reads its command-line options from environment variables when the corresponding flag is omitted: `PNPR_CONFIG`, `PNPR_LISTEN`, `PNPR_STORAGE`, `PNPR_CACHE`, `PNPR_PUBLIC_URL`, `PNPR_PACKUMENT_TTL_SECS`, `PNPR_OSV`, `PNPR_OSV_DB`, `PNPR_DISABLE_REGISTRY`, `PNPR_DISABLE_RESOLVER`, and `PNPR_DISABLE_ARTIFACTS`. Boolean flags accept common truthy and falsy strings such as `true`, `1`, and `yes`.
