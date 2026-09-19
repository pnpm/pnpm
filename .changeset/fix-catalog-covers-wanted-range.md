---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm add <pkg>@<range>` now reuses an existing catalog entry when the catalog range covers the wanted range. Adding a range used to fail with `ERR_PNPM_CATALOG_VERSION_MISMATCH` under `catalogMode: strict`. Under `catalogMode: prefer` it wrote a direct range into the manifest [pnpm/pnpm#14865](https://github.com/pnpm/pnpm/issues/14865).
