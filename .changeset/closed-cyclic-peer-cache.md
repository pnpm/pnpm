---
"pacquet": patch
---

Fixed `pnpm install --lockfile-only` writing a lockfile that referenced a missing peer-suffixed snapshot when an npm-aliased dependency took part in a cyclic peer dependency graph. The following `pnpm install --frozen-lockfile` failed with `ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY` [#14449](https://github.com/pnpm/pnpm/issues/14449).
