---
"@pnpm/installing.deps-installer": patch
"@pnpm/pnpr.client": patch
"@pnpm/pnpr": patch
"pnpm": patch
"pacquet": patch
---

Forward `patchedDependencies` hashes and `packageExtensions` to pnpr so server-side resolution preserves patches and package extensions in the lockfile and installed packages.
