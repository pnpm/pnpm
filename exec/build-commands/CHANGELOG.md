# @pnpm/exec.build-commands

## 1000.0.3

### Patch Changes

- Updated dependencies [f6006f2]
  - @pnpm/config@1002.3.0
  - @pnpm/plugin-commands-rebuild@1001.1.6

## 1000.0.2

### Patch Changes

- afbb654: `pnpm approve-builds` should work, when executed from a subdirectory of a workspace [#9042](https://github.com/pnpm/pnpm/issues/9042).

## 1000.0.1

### Patch Changes

- 5d7192c: `approve-builds` command gets the auto-ignore build list and exits early when it is an empty array [#9024](https://github.com/pnpm/pnpm/pull/9024).
- a2a4509: Sort the package names in the "pnpm.onlyBuiltDependencies" list saved by `pnpm approve-builds`.
- Updated dependencies [1e229d7]
  - @pnpm/read-project-manifest@1000.0.5
  - @pnpm/plugin-commands-rebuild@1001.1.5
  - @pnpm/config@1002.2.1

## 1000.0.0

### Major Changes

- 961dc5d: Added a new command for printing the list of dependencies with ignored build scripts: `pnpm ignored-builds` [#8963](https://github.com/pnpm/pnpm/pull/8963).
- 961dc5d: Added a new command for approving dependencies for running scripts during installation: `pnpm approve-builds` [#8963](https://github.com/pnpm/pnpm/pull/8963).

### Patch Changes

- Updated dependencies [f3ffaed]
- Updated dependencies [c96eb2b]
  - @pnpm/config@1002.2.0
  - @pnpm/plugin-commands-rebuild@1001.1.4
  - @pnpm/modules-yaml@1000.1.2
  - @pnpm/read-project-manifest@1000.0.4
