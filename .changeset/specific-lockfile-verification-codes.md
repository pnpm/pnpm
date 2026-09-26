---
"pacquet": patch
---

Lockfile verification now fails with `ERR_PNPM_TARBALL_URL_MISMATCH`, `ERR_PNPM_TARBALL_REVISION_MISMATCH`, or `ERR_PNPM_MISSING_NAMED_REGISTRY` when every rejected entry failed that check. These failures were reported as the generic `ERR_PNPM_LOCKFILE_RESOLUTION_VERIFICATION`.
