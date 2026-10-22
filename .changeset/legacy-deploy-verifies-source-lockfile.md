---
"pacquet": patch
---

`pnpm deploy --legacy` now checks the workspace lockfile against `minimumReleaseAge` and the other resolution policies, as `pnpm install` does, and fails with `ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION` when a locked version is too new. It used to deploy an older version that is not in the lockfile without a warning [#17209](https://github.com/pnpm/pnpm/issues/17209).
