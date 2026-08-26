---
"pacquet": minor
---

`pnpm` now supports per-branch lockfiles in its Rust engine:

- `gitBranchLockfile` gives each git branch its own `pnpm-lock.<branch>.yaml`, so two branches can hold different resolutions without conflicting on one file. A branch that has no lockfile yet installs against the shared `pnpm-lock.yaml`.
- `mergeGitBranchLockfiles` (and the `--merge-git-branch-lockfiles` flag on `pnpm install`) folds every branch lockfile back into `pnpm-lock.yaml` and deletes them, which is what merging a branch into the mainline needs.
- `mergeGitBranchLockfilesBranchPattern` (and `--merge-git-branch-lockfiles-branch-pattern`) names the branches that merge automatically, so a mainline branch does not have to pass the flag by hand [#12042](https://github.com/pnpm/pnpm/issues/12042).
