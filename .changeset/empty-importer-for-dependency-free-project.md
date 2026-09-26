---
"pacquet": patch
---

`pnpm install` now records an empty importer entry for a workspace package with no dependencies. A package added after the lockfile was written used to be left out, so the next `--frozen-lockfile` install failed with `ERR_PNPM_PACKAGE_MANAGER_NO_IMPORTER` [pnpm/pnpm#15875](https://github.com/pnpm/pnpm/issues/15875).
