---
"@pnpm/network.git-utils": patch
"@pnpm/lockfile.fs": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --frozen-lockfile` now works on a detached HEAD when `gitBranchLockfile` is enabled. The install now reads the lockfiles of the local and remote-tracking branches that contain the checked-out commit. It still writes the shared `pnpm-lock.yaml` [#7672](https://github.com/pnpm/pnpm/issues/7672).
