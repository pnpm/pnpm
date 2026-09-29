---
id: git_branch_lockfiles
title: Git Branch Lockfiles
---

Git branch lockfiles allows you to totally avoid lockfile merge conflicts and solve it later.

## Use git branch lockfiles

You can turn on this feature by configuring the `.npmrc` file.

```ini
git-branch-lockfile=true
```

By doing this, lockfile name will be generated based on the current branch name.

For instance, the current branch name is `feature-1`. Then, the generated lockfile name will
be `pnpm-lock.feature-1.yaml`. You can commit it to the Git, and merge all git branch lockfiles later.

```
- <project_folder>
|- pnpm-lock.yaml
|- pnpm-lock.feature-1.yaml
|- pnpm-lock.<branch_name>.yaml
```

:::note

`feature/1` is special in that the `/` is automatically converted to `!`, so the corresponding
lockfile name would be `pnpm-lock.feature!1.yaml`.

:::

## Merge git branch lockfiles

### `pnpm install --merge-git-branch-lockfiles`

To merge all git branch lockfiles, just specify `--merge-git-branch-lockfiles` to `pnpm install` command.

After that, all git branch lockfiles will be merged into one `pnpm-lock.yaml`


### Branch Matching

pnpm allows you to specify `--merge-git-branch-lockfiles` by matching the current branch name.

For instance, by the following setting in `.npmrc` file, `pnpm install` will merge all git branch lockfiles when
running in the `main` branch and the branch name starts with `release`.

```ini
merge-git-branch-lockfiles-branch-pattern[]=main
merge-git-branch-lockfiles-branch-pattern[]=release*
```
