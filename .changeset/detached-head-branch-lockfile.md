---
"@pnpm/network.git-utils": patch
"@pnpm/lockfile.fs": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --frozen-lockfile` no longer fails on a detached HEAD when `gitBranchLockfile` is enabled. The install now reads the lockfiles of the branches containing the checked-out commit, and still writes the shared `pnpm-lock.yaml` [#7672](https://github.com/pnpm/pnpm/issues/7672).
