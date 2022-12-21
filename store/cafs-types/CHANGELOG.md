# @pnpm/cafs-types

## 1.2.0

### Minor Changes

- 2458741fa: A new option added to package importer for keeping modules directory: `keepModulesDir`. When this is set to true, if a package already exist at the target location and it has a node_modules directory, then that node_modules directory is moved to the newly imported dependency. This is only needed when node-linker=hoisted is used.

## 1.1.0

### Minor Changes

- 745143e79: Extend cafs with `getFilePathByModeInCafs`.

## 1.0.0

### Major Changes

- 32915f0e4: Refactor cafs types into separate package and add additional properties including `cafsDir` and `getFilePathInCafs`.
