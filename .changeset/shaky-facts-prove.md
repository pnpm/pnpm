---
"@pnpm/npm-resolver": patch
---

Offline mode should only resolve to versions present in the local cache, preventing ERR_PNPM_NO_OFFLINE_TARBALL when uncached versions exist in the metadata.
