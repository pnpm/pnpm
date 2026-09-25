---
"pacquet": patch
"pnpm": patch
---

`pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` when the pinned pnpm version recorded in `pnpm-lock.yaml` has to be re-resolved before it can be installed. It runs the pnpm version the lockfile pins and leaves the lockfile unchanged [#14124](https://github.com/pnpm/pnpm/issues/14124).
