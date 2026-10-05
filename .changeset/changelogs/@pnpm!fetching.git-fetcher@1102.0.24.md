## 1102.0.24

### Patch Changes

- pnpm now rejects a git dependency whose lockfile repository is empty, begins with `-`, or contains a null byte. Git can no longer read such a value as a command-line option [pnpm/tasks#84](https://github.com/pnpm/tasks/issues/84).

- Updated dependencies:
  - @pnpm/error@1100.2.2
  - @pnpm/exec.prepare-package@1100.0.42
  - @pnpm/fs.packlist@1100.0.8
  - @pnpm/resolving.git-resolver@1100.1.27
  - @pnpm/store.index@1101.0.1
