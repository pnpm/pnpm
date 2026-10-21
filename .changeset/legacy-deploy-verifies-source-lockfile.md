---
"pacquet": patch
---

`pnpm deploy --legacy` now checks the workspace lockfile against `minimumReleaseAge` and the other resolution policies, and fails with `ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION` when a locked version is rejected. It used to deploy a different version that is not in the lockfile without a warning [#17209](https://github.com/pnpm/pnpm/issues/17209).
