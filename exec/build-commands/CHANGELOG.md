# @pnpm/exec.build-commands

## 1001.0.3

### Patch Changes

- 1e6ae3e: When executing the `approve-builds` command, if package.json contains `onlyBuiltDependencies` or `ignoredBuiltDependencies`, the selected dependency package will continue to be written into `package.json`.
- Updated dependencies [c3aa4d8]
  - @pnpm/config@1002.5.1
  - @pnpm/plugin-commands-rebuild@1002.0.3

## 1001.0.2

### Patch Changes

- 8b3cfe2: fix: don't abort approve-builds command or err when manifest doesn't exist [#9198](https://github.com/pnpm/pnpm/pull/9198)
- Updated dependencies [d965748]
  - @pnpm/config@1002.5.0
  - @pnpm/plugin-commands-rebuild@1002.0.2
  - @pnpm/modules-yaml@1000.1.4
  - @pnpm/read-project-manifest@1000.0.7
  - @pnpm/workspace.manifest-writer@1000.0.2

## 1001.0.1

### Patch Changes

- Updated dependencies [23754c7]
- Updated dependencies [1c2eb8c]
  - @pnpm/workspace.manifest-writer@1000.0.1
  - @pnpm/config@1002.4.1
  - @pnpm/plugin-commands-rebuild@1002.0.1

## 1001.0.0

### Major Changes

- 8fcc221: Read `onlyBuiltDependencies` and `ignoredBuiltDependencies` from `options`.

### Patch Changes

- Updated dependencies [8fcc221]
- Updated dependencies [8fcc221]
- Updated dependencies [8fcc221]
- Updated dependencies [e32b1a2]
- Updated dependencies [8fcc221]
  - @pnpm/workspace.manifest-writer@1000.0.0
  - @pnpm/config@1002.4.0
  - @pnpm/plugin-commands-rebuild@1002.0.0
  - @pnpm/modules-yaml@1000.1.3
  - @pnpm/read-project-manifest@1000.0.6

## 1000.1.1

### Patch Changes

- 546ab37: Throws an error when the value provided by the `--allow-build` option overlaps with the `pnpm.ignoredBuildDependencies` list [#9105](https://github.com/pnpm/pnpm/pull/9105).
- Updated dependencies [fee898f]
  - @pnpm/config@1002.3.1
  - @pnpm/plugin-commands-rebuild@1001.1.8

## 1000.1.0

### Minor Changes

- 4aa6d45: `pnpm approve-builds --global` works now for allowing dependencies of globally installed packages to run postinstall scripts.

### Patch Changes

- @pnpm/plugin-commands-rebuild@1001.1.7

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
