---
"@pnpm/lockfile-utils": patch
"@pnpm/resolver-base": patch
---

Tarball resolutions in pnpm-lock.yaml will no longer contain a `registry` field, as it was unused.
