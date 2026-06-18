# @pnpm/git-utils

## 1100.0.2

### Patch Changes

- 61969fb: Fix `pnpm install` with `optimisticRepeatInstall` incorrectly reporting `Already up to date` when `pnpm-lock.yaml` changed but project manifests did not. This affected workflows such as checking out or restoring only the lockfile [#12100](https://github.com/pnpm/pnpm/issues/12100).

  Also fixes `checkDepsStatus` to use the correct lockfile path when `useGitBranchLockfile` is enabled, so the optimistic fast-path and lockfile modification detection work with `pnpm-lock.<branch>.yaml` files instead of always stat'ing `pnpm-lock.yaml`. Merge-conflict detection now reads the resolved lockfile name as well, and with `mergeGitBranchLockfiles` enabled every `pnpm-lock.*.yaml` is scanned for modifications and conflicts. The git branch is now resolved by reading `.git/HEAD` directly (no process spawn) and uses the workspace directory rather than `process.cwd()`.

## 1100.0.1

### Patch Changes

- 184ce26: Fix the package name in README.md.

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

## 2.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

## 1.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

## 0.1.0

### Minor Changes

- 56cf04cb3: New settings added: use-git-branch-lockfile, merge-git-branch-lockfiles, merge-git-branch-lockfiles-branch-pattern.
