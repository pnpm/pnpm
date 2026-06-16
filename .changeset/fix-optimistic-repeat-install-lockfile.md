---
"@pnpm/deps.status": patch
"pnpm": patch
---

Fix `pnpm install` with `optimisticRepeatInstall` incorrectly reporting `Already up to date` when `pnpm-lock.yaml` changed but project manifests did not. This affected workflows such as checking out or restoring only the lockfile [#12100](https://github.com/pnpm/pnpm/issues/12100).

Also fixes `checkDepsStatus` to use the correct lockfile path when `useGitBranchLockfile` is enabled, so the optimistic fast-path and lockfile modification detection work with `pnpm-lock.<branch>.yaml` files instead of always stat'ing `pnpm-lock.yaml`.
