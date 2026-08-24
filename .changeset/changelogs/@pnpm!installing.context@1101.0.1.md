## 1101.0.1

### Patch Changes

- Fixed `pnpm install --merge-git-branch-lockfiles --frozen-lockfile` failing with `ERR_PNPM_OUTDATED_LOCKFILE` when a branch lockfile predates the removal of a dependency, or its move to another dependency group [#13966](https://github.com/pnpm/pnpm/issues/13966). A dependency that no project declares anymore is no longer reinstated by the merge, and the packages it was the only path to are dropped with it.

- Updated dependencies:
  - @pnpm/installing.read-projects-context@1101.0.1
  - @pnpm/lockfile.fs@1100.2.4
