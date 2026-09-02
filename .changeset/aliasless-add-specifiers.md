---
"pacquet": patch
---

`pnpm add <local directory>`, `pnpm add <local tarball>`, `pnpm add file:<path>` and `pnpm add <tarball URL>` work again: a specifier given without a `<name>@` prefix is no longer read as a registry package name and rejected with `ERR_PNPM_PACKAGE_MANAGER_ADD_RESOLVE_LATEST` [#14437](https://github.com/pnpm/pnpm/issues/14437).
