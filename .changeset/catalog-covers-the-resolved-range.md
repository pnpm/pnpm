---
"pacquet": patch
---

`pnpm add <pkg>` without a version now uses the catalog entry when the workspace already catalogs that package. Naming no version asks for whatever the workspace agreed on, so the entry stands even where it differs from the range `latest` would produce. `catalogMode: strict` used to fail with `ERR_PNPM_CATALOG_VERSION_MISMATCH` and `catalogMode: prefer` wrote a direct range into the manifest [pnpm/pnpm#14865](https://github.com/pnpm/pnpm/issues/14865).
