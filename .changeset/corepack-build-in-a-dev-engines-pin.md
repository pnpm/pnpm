---
"pacquet": patch
---

A `+<algorithm>.<hash>` build in a `devEngines.packageManager` version no longer makes `pnpm install --frozen-lockfile` fail with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` on a lockfile a plain install kept rewriting identically [#14124](https://github.com/pnpm/pnpm/issues/14124).
