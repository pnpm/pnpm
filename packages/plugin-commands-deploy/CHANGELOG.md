# @pnpm/plugin-commands-deploy

## 1.1.0

### Minor Changes

- 2aa22e4b1: Set `NODE_PATH` when `preferSymlinkedExecutables` is enabled.

### Patch Changes

- Updated dependencies [2aa22e4b1]
  - @pnpm/plugin-commands-installation@10.6.0
  - @pnpm/cli-utils@0.7.31

## 1.0.19

### Patch Changes

- @pnpm/plugin-commands-installation@10.5.8
- @pnpm/cli-utils@0.7.30

## 1.0.18

### Patch Changes

- @pnpm/plugin-commands-installation@10.5.7
- @pnpm/fs.indexed-pkg-importer@1.1.1
- @pnpm/cli-utils@0.7.29
- @pnpm/directory-fetcher@3.1.1

## 1.0.17

### Patch Changes

- Updated dependencies [9faf0221d]
- Updated dependencies [07bc24ad1]
  - @pnpm/plugin-commands-installation@10.5.6
  - @pnpm/directory-fetcher@3.1.1
  - @pnpm/cli-utils@0.7.28
  - @pnpm/fs.indexed-pkg-importer@1.1.1

## 1.0.16

### Patch Changes

- Updated dependencies [23984abd1]
  - @pnpm/directory-fetcher@3.1.0
  - @pnpm/fs.indexed-pkg-importer@1.1.1
  - @pnpm/plugin-commands-installation@10.5.5
  - @pnpm/cli-utils@0.7.27

## 1.0.15

### Patch Changes

- @pnpm/plugin-commands-installation@10.5.4
- @pnpm/fs.indexed-pkg-importer@1.1.0
- @pnpm/directory-fetcher@3.0.10

## 1.0.14

### Patch Changes

- Updated dependencies [39c040127]
- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
  - @pnpm/directory-fetcher@3.0.10
  - @pnpm/plugin-commands-installation@10.5.3
  - @pnpm/fs.indexed-pkg-importer@1.1.0
  - @pnpm/cli-utils@0.7.26

## 1.0.13

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/cli-utils@0.7.25
  - @pnpm/plugin-commands-installation@10.5.2
  - @pnpm/fs.indexed-pkg-importer@1.0.1
  - @pnpm/directory-fetcher@3.0.9

## 1.0.12

### Patch Changes

- @pnpm/plugin-commands-installation@10.5.1

## 1.0.11

### Patch Changes

- Updated dependencies [cac34ad69]
- Updated dependencies [99019e071]
  - @pnpm/plugin-commands-installation@10.5.0
  - @pnpm/cli-utils@0.7.24

## 1.0.10

### Patch Changes

- c7519ad6a: **pnpm deploy**: accept absolute paths and use cwd instead of workspaceDir for deploy target directory [#4980](https://github.com/pnpm/pnpm/issues/4980).
  - @pnpm/plugin-commands-installation@10.4.2
  - @pnpm/cli-utils@0.7.23
  - @pnpm/fs.indexed-pkg-importer@1.0.0
  - @pnpm/directory-fetcher@3.0.8

## 1.0.9

### Patch Changes

- 107d01109: `pnpm deploy` should inject local dependencies of all types (dependencies, optionalDependencies, devDependencies) [#5078](https://github.com/pnpm/pnpm/issues/5078).
  - @pnpm/cli-utils@0.7.22
  - @pnpm/directory-fetcher@3.0.8
  - @pnpm/plugin-commands-installation@10.4.1

## 1.0.8

### Patch Changes

- 0569f1022: `pnpm deploy` should not modify the lockfile [#5071](https://github.com/pnpm/pnpm/issues/5071)
- 0569f1022: `pnpm deploy` should not fail in CI [#5071](https://github.com/pnpm/pnpm/issues/5071)
- Updated dependencies [0569f1022]
  - @pnpm/plugin-commands-installation@10.4.0
  - @pnpm/cli-utils@0.7.21

## 1.0.7

### Patch Changes

- 31e73ba77: `pnpm deploy` should include all dependencies by default [#5035](https://github.com/pnpm/pnpm/issues/5035).
- Updated dependencies [406656f80]
  - @pnpm/plugin-commands-installation@10.3.10
  - @pnpm/cli-utils@0.7.20

## 1.0.6

### Patch Changes

- @pnpm/plugin-commands-installation@10.3.9
- @pnpm/cli-utils@0.7.19

## 1.0.5

### Patch Changes

- @pnpm/plugin-commands-installation@10.3.8

## 1.0.4

### Patch Changes

- @pnpm/cli-utils@0.7.18
- @pnpm/plugin-commands-installation@10.3.7

## 1.0.3

### Patch Changes

- Updated dependencies [b55b3782d]
  - @pnpm/plugin-commands-installation@10.3.6

## 1.0.2

### Patch Changes

- Updated dependencies [5f643f23b]
  - @pnpm/cli-utils@0.7.17
  - @pnpm/directory-fetcher@3.0.7
  - @pnpm/plugin-commands-installation@10.3.5

## 1.0.1

### Patch Changes

- f4248b514: Changes deployment directories to be created recursively
  - @pnpm/plugin-commands-installation@10.3.4

## 1.0.0

### Major Changes

- 7922d6314: A new experimental command added: `pnpm deploy`. The deploy command takes copies a project from a workspace and installs all of its production dependencies (even if some of those dependencies are other projects from the workspace).

  For example, the new command will deploy the project named `foo` to the `dist` directory in the root of the workspace:

  ```
  pnpm --filter=foo deploy dist
  ```

### Patch Changes

- Updated dependencies [7922d6314]
  - @pnpm/fs.indexed-pkg-importer@1.0.0
  - @pnpm/plugin-commands-installation@10.3.3
