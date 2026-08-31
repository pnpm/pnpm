---
"@pnpm/installing.context": patch
"@pnpm/lockfile.fs": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm install --merge-git-branch-lockfiles --frozen-lockfile` failing with `ERR_PNPM_OUTDATED_LOCKFILE` when a branch lockfile predates the removal of a dependency, or its move to another dependency group [#13966](https://github.com/pnpm/pnpm/issues/13966). A dependency that no project declares anymore is no longer reinstated by the merge, and the packages it was the only path to are dropped with it.
