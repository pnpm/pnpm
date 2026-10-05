---
"pacquet": patch
---

`pnpm install --fix-lockfile` repairs a lockfile whose importer references a package that has no snapshot entry, as left by a badly merged lockfile. It failed with `ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY` since 12.8.0 [#16618](https://github.com/pnpm/pnpm/issues/16618).
