# pkgs-graph

## 7.0.1

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- 42c1ea1c0: Update validate-npm-package-name to v4.

## 7.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/resolve-workspace-range@3.0.0

## 6.1.3

### Patch Changes

- f82cc7f77: fix: when set workspace protocol the pkgs in workspace without version not ignore

## 6.1.2

### Patch Changes

- a1a03d145: Import only the required functions from ramda.

## 6.1.1

### Patch Changes

- 1084ca1a7: Include dependencies with workspace version aliases in graph

## 6.1.0

### Minor Changes

- dfdf669e6: Add new cli arg --filter-prod. --filter-prod acts the same as --filter, but it omits devDependencies when building dependencies

### Patch Changes

- Updated dependencies [85fb21a83]
  - @pnpm/resolve-workspace-range@2.1.0

## 6.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
  - @pnpm/resolve-workspace-range@2.0.0

## 5.2.0

### Minor Changes

- e37a5a175: Support linkedWorkspacePackages=false.

## 5.1.6

### Patch Changes

- @pnpm/resolve-workspace-range@1.0.2
