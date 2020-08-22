# @pnpm/lifecycle

## 9.2.3

### Patch Changes

- Updated dependencies [9f5803187]
  - @pnpm/read-package-json@3.1.4

## 9.2.2

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.

## 9.2.1

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0

## 9.2.0

### Minor Changes

- 76aaead32: Added an option for silent execution: opts.silent.

## 9.1.3

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/core-loggers@4.1.2
  - @pnpm/read-package-json@3.1.3

## 9.1.2

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/core-loggers@4.1.1
  - @pnpm/read-package-json@3.1.2

## 9.1.1

### Patch Changes

- d3ddd023c: Update p-limit to v3.
- 68d8dc68f: Update node-gyp to v7.
- Updated dependencies [2ebb7af33]
  - @pnpm/core-loggers@4.1.0

## 9.1.0

### Minor Changes

- 8094b2a62: Run lifecycle scripts with the PNPM_SCRIPT_SRC_DIR env variable set. This new env variable contains the directory of the package.json file that contains the executed lifecycle script.

## 9.0.0

### Major Changes

- e3990787a: Rename NodeModules to Modules in option names.

### Minor Changes

- f35a3ec1c: Don't execute lifecycle scripts that are meant to prevent the usage of npm or Yarn.

### Patch Changes

- Updated dependencies [da091c711]
  - @pnpm/types@6.0.0
  - @pnpm/core-loggers@4.0.2
  - @pnpm/read-package-json@3.1.1

## 9.0.0-alpha.1

### Major Changes

- e3990787: Rename NodeModules to Modules in option names.

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/core-loggers@4.0.2-alpha.0
  - @pnpm/read-package-json@3.1.1-alpha.0

## 8.2.0-alpha.0

### Minor Changes

- f35a3ec1c: Don't execute lifecycle scripts that are meant to prevent the usage of npm or Yarn.

## 8.2.0

### Minor Changes

- 2ec4c4eb9: Don't execute lifecycle scripts that are meant to prevent the usage of npm or Yarn.
