---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm add <pkg>` without a version now reuses an existing catalog entry when the catalog range covers the range the package resolves to. Before, only a concrete version could match a catalog range. `catalogMode: strict` failed with `ERR_PNPM_CATALOG_VERSION_MISMATCH` and `catalogMode: prefer` wrote the range into the manifest [pnpm/pnpm#14865](https://github.com/pnpm/pnpm/issues/14865).
