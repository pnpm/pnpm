---
"pacquet": patch
---

`pnpm dlx <pkg>@catalog:` now resolves the specifier through the calling workspace's catalogs instead of failing with `ERR_PNPM_CATALOG_ENTRY_NOT_FOUND_FOR_SPEC` [#14294](https://github.com/pnpm/pnpm/issues/14294).
