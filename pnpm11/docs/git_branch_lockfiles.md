---
id: git_branch_lockfiles
title: Git Branch Lockfiles
---

Git branch lockfiles allows you to totally avoid lockfile merge conflicts and solve it later.

## Use git branch lockfiles

You can turn on this feature by configuring the `pnpm-workspace.yaml` file.

```yaml title="pnpm-workspace.yaml"
gitBranchLockfile: true
```

By doing this, lockfile name will be generated based on the current branch name.

For instance, the current branch name is `feature-1`. Then, the generated lockfile name will
be `pnpm-lock.feature-1.yaml`. You can commit it to the Git, and merge all git branch lockfiles later.

```text
- <project_folder>
|- pnpm-lock.yaml
|- pnpm-lock.feature-1.yaml
|- pnpm-lock.<branch_name>.yaml
```

:::note

Every character in the branch name other than ASCII letters, digits, `_`, `.`, and `-` is converted to `!`,
and the name is lowercased. So the lockfile of the `Feature/1` branch is `pnpm-lock.feature!1.yaml`.
A character outside the Basic Multilingual Plane, such as an emoji, becomes `!!`.

:::

## Merge git branch lockfiles

### `pnpm install --merge-git-branch-lockfiles`

To merge all git branch lockfiles, just specify `--merge-git-branch-lockfiles` to `pnpm install` command.

After that, all git branch lockfiles will be merged into one `pnpm-lock.yaml`, and the branch lockfiles are deleted.


### Branch matching

pnpm allows you to specify `--merge-git-branch-lockfiles` by matching the current branch name.

For instance, by the following setting in `pnpm-workspace.yaml` file, `pnpm install` will merge all git branch lockfiles when
running in the `main` branch or in a branch whose name starts with `release`.

```yaml title="pnpm-workspace.yaml"
mergeGitBranchLockfilesBranchPattern:
- main
- release*
```

See [`gitBranchLockfile`](./settings/store.md#gitbranchlockfile) and [`mergeGitBranchLockfilesBranchPattern`](./settings/store.md#mergegitbranchlockfilesbranchpattern) for the setting reference.
