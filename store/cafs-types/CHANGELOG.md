# @pnpm/cafs-types

## 3.0.1

### Patch Changes

- 4a1a9431d: Breaking change to the `directory-fetcher` API.

## 3.0.0

### Major Changes

- f2009d175: Import packages synchronously.
- 083bbf590: Breaking changes to the API.

## 2.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

## 1.2.0

### Minor Changes

- 2458741fa: A new option added to package importer for keeping modules directory: `keepModulesDir`. When this is set to true, if a package already exist at the target location and it has a node_modules directory, then that node_modules directory is moved to the newly imported dependency. This is only needed when node-linker=hoisted is used.

## 1.1.0

### Minor Changes

- 745143e79: Extend cafs with `getFilePathByModeInCafs`.

## 1.0.0

### Major Changes

- 32915f0e4: Refactor cafs types into separate package and add additional properties including `cafsDir` and `getFilePathInCafs`.
