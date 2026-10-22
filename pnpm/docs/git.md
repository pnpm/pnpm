---
id: git
title: Working with Git
---

## Lockfiles

You should always commit the lockfile (`pnpm-lock.yaml`). This is for a
multitude of reasons, the primary of which being:
- it enables faster installation for CI and production environments, due to
being able to skip package resolution
- it enforces consistent installations and resolution between development,
testing, and production environments, meaning the packages used in testing
and production will be exactly the same as when you developed your project

### Merge conflicts

pnpm can automatically resolve merge conflicts in `pnpm-lock.yaml`.
If you have conflicts, run `pnpm install` and commit the changes.

If both sides of the conflict are valid lockfiles, pnpm merges them. Otherwise it fails with the parse error. Review the result before committing.

To avoid lockfile merge conflicts, use [Git branch lockfiles](./git_branch_lockfiles.md).
