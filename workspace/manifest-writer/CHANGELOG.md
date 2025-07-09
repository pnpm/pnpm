# @pnpm/workspace.manifest-writer

## 1000.2.1

### Patch Changes

- 95a9b82: Sort keys in `pnpm-workspace.yaml` with deep [#9701](https://github.com/pnpm/pnpm/pull/9701).

## 1000.2.0

### Minor Changes

- c8341cc: Added two new CLI options (`--save-catalog` and `--save-catalog-name=<name>`) to `pnpm add` to save new dependencies as catalog entries. `catalog:` or `catalog:<name>` will be added to `package.json` and the package specifier will be added to the `catalogs` or `catalog[<name>]` object in `pnpm-workspace.yaml` [#9425](https://github.com/pnpm/pnpm/issues/9425).

### Patch Changes

- Updated dependencies [c8341cc]
  - @pnpm/workspace.read-manifest@1000.2.0

## 1000.1.4

### Patch Changes

- Updated dependencies [c00360b]
  - @pnpm/object.key-sorting@1000.0.1
  - @pnpm/workspace.read-manifest@1000.1.5

## 1000.1.3

### Patch Changes

- 2bcb402: Sort keys in `pnpm-workspace.yaml` [#9453](https://github.com/pnpm/pnpm/pull/9453).

## 1000.1.2

### Patch Changes

- @pnpm/workspace.read-manifest@1000.1.4

## 1000.1.1

### Patch Changes

- ead11ad: Don't wrap lines in `pnpm-workspace.yaml`.
  - @pnpm/workspace.read-manifest@1000.1.3

## 1000.1.0

### Minor Changes

- 3a90ec1: `pnpm config delete --location=project` The setting in `pnpm-workspace.yaml` file will be deleted if no `.npmrc` file is present in the directory

### Patch Changes

- @pnpm/workspace.read-manifest@1000.1.2

## 1000.0.2

### Patch Changes

- @pnpm/workspace.read-manifest@1000.1.1

## 1000.0.1

### Patch Changes

- 23754c7: Fix the update of `pnpm-workspace.yaml` by the `pnpm approve-builds` command [#9168](https://github.com/pnpm/pnpm/issues/9168).

## 1000.0.0

### Major Changes

- 8fcc221: Initial release.
- 8fcc221: Initial release.

### Patch Changes

- Updated dependencies [8fcc221]
- Updated dependencies [8fcc221]
  - @pnpm/workspace.read-manifest@1000.1.0
