---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm add <pkg>@<range>` now reuses an existing catalog entry when the catalog range covers the wanted range. Before, only a concrete version could match a catalog range, so `catalogMode: strict` failed with `ERR_PNPM_CATALOG_VERSION_MISMATCH` and `catalogMode: prefer` wrote the range into the manifest. On pnpm 12 this also affected `pnpm add <pkg>` without a version, which resolves to a range [pnpm/pnpm#14865](https://github.com/pnpm/pnpm/issues/14865).
