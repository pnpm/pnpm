---
"pacquet": patch
---

`pnpm add <pkg>` now reuses an existing catalog entry when the catalog range covers the range the package resolves to. `pnpm add <pkg>@<range>` does the same for a range you pass yourself. Adding without an explicit version used to fail with `ERR_PNPM_CATALOG_VERSION_MISMATCH` under `catalogMode: strict`. Under `catalogMode: prefer` it wrote a direct range into the manifest [pnpm/pnpm#14865](https://github.com/pnpm/pnpm/issues/14865).
