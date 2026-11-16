---
"pacquet": patch
---

`pnpm update` now moves an `engines.pnpm` range along with the pnpm version in `packageManager` or `devEngines.packageManager`, so the next `pnpm install` no longer fails with `ERR_PNPM_UNSUPPORTED_ENGINE` [#18142](https://github.com/pnpm/pnpm/issues/18142).
