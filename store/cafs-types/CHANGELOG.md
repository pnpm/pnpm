# @pnpm/cafs-types

## 6.0.0

### Major Changes

- 099e6af: Changed the structure of the index files in the store to store side effects cache information more efficiently. In the new version, side effects do not list all the files of the package but just the differences [#8636](https://github.com/pnpm/pnpm/pull/8636).

## 5.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Minor Changes

- 730929e: Add a field named `ignoredOptionalDependencies`. This is an array of strings. If an optional dependency has its name included in this array, it will be skipped.

## 4.0.0

### Major Changes

- 9caa33d53: `fromStore` replaced with `resolvedFrom`.

## 3.1.0

### Minor Changes

- 03cdccc6e: New option added: disableRelinkFromStore.

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
