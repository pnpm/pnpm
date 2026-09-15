---
"pacquet": patch
---

`pnpm update --no-save` no longer rewrites the specifier of a dependency it is not updating. An override-applied specifier was replaced with the range the manifest declares, and the next `pnpm install --frozen-lockfile` failed with `ERR_PNPM_OUTDATED_LOCKFILE` [#14836](https://github.com/pnpm/pnpm/issues/14836).
