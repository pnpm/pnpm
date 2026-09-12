---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

`pnpm add <pkg>@<version>` and `pnpm update <pkg>@<version>` under `catalogMode: strict` no longer fail with `ERR_PNPM_CATALOG_VERSION_MISMATCH` when the catalog entry is a range that the wanted version satisfies. The dependency keeps using the catalog; only a version that really falls outside the catalog's range is rejected [#13715](https://github.com/pnpm/pnpm/issues/13715).
