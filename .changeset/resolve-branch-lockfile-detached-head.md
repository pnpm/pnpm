---
"@pnpm/lockfile.fs": patch
"@pnpm/network.git-utils": patch
"pnpm": patch
"pacquet": patch
---

`git-branch-lockfile` now resolves the matching branch lockfile on a detached HEAD in CI and when candidate branches contain HEAD [pnpm/pnpm#7672](https://github.com/pnpm/pnpm/issues/7672).
