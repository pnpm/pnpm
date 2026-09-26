---
"@pnpm/lockfile.merger": patch
"@pnpm/lockfile.types": patch
"pnpm": patch
"pacquet": patch
---

Merging lockfiles now preserves recorded configuration fields such as `overrides`, `neverBuiltDependencies`, `patchedDependencies`, `packageExtensionsChecksum`, `settings`, and `catalogs` [pnpm/pnpm#8366](https://github.com/pnpm/pnpm/issues/8366).
