---
"pacquet": patch
---

`pnpm install --frozen-lockfile` now accepts a lockfile that has no importer entry for a workspace package without dependencies. Such a package added after the lockfile was written made the install fail with `ERR_PNPM_PACKAGE_MANAGER_NO_IMPORTER` [pnpm/pnpm#15875](https://github.com/pnpm/pnpm/issues/15875).
