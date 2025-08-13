# @pnpm/workspace.read-manifest

## 1000.2.2

### Patch Changes

- Updated dependencies [d1edf73]
- Updated dependencies [86b33e9]
  - @pnpm/constants@1001.3.0
  - @pnpm/error@1000.0.4

## 1000.2.1

### Patch Changes

- Updated dependencies [1a07b8f]
- Updated dependencies [1a07b8f]
  - @pnpm/types@1000.7.0
  - @pnpm/constants@1001.2.0
  - @pnpm/error@1000.0.3

## 1000.2.0

### Minor Changes

- c8341cc: Added two new CLI options (`--save-catalog` and `--save-catalog-name=<name>`) to `pnpm add` to save new dependencies as catalog entries. `catalog:` or `catalog:<name>` will be added to `package.json` and the package specifier will be added to the `catalogs` or `catalog[<name>]` object in `pnpm-workspace.yaml` [#9425](https://github.com/pnpm/pnpm/issues/9425).

## 1000.1.5

### Patch Changes

- Updated dependencies [5ec7255]
  - @pnpm/types@1000.6.0

## 1000.1.4

### Patch Changes

- Updated dependencies [5b73df1]
  - @pnpm/types@1000.5.0

## 1000.1.3

### Patch Changes

- Updated dependencies [750ae7d]
  - @pnpm/types@1000.4.0

## 1000.1.2

### Patch Changes

- Updated dependencies [5f7be64]
- Updated dependencies [5f7be64]
  - @pnpm/types@1000.3.0

## 1000.1.1

### Patch Changes

- Updated dependencies [a5e4965]
  - @pnpm/types@1000.2.1

## 1000.1.0

### Minor Changes

- 8fcc221: Extend WorkspaceManifest with PnpmSettings.
- 8fcc221: The `packages` field in `pnpm-workspace.yaml` became optional.

### Patch Changes

- Updated dependencies [8fcc221]
  - @pnpm/types@1000.2.0

## 1000.0.2

### Patch Changes

- Updated dependencies [9a44e6c]
  - @pnpm/constants@1001.1.0
  - @pnpm/error@1000.0.2

## 1000.0.1

### Patch Changes

- Updated dependencies [d2e83b0]
- Updated dependencies [a76da0c]
  - @pnpm/constants@1001.0.0
  - @pnpm/error@1000.0.1

## 2.2.2

### Patch Changes

- Updated dependencies [19d5b51]
- Updated dependencies [8108680]
- Updated dependencies [c4f5231]
  - @pnpm/constants@10.0.0
  - @pnpm/error@6.0.3

## 2.2.1

### Patch Changes

- Updated dependencies [83681da]
  - @pnpm/constants@9.0.0
  - @pnpm/error@6.0.2

## 2.2.0

### Minor Changes

- 9c63679: The `readWorkspaceManifest` function now parses and validates [pnpm catalogs](https://github.com/pnpm/rfcs/pull/1) configs if present.

## 2.1.0

### Minor Changes

- 5d1ed94: The type definition for the `packages` field of the `WorkspaceManifest` is now non-null. The `readWorkspaceManifest` function expects this field to be present and throws an error otherwise.

## 2.0.1

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1

## 2.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [3ded840]
- Updated dependencies [c692f80]
- Updated dependencies [43cdd87]
- Updated dependencies [d381a60]
  - @pnpm/error@6.0.0
  - @pnpm/constants@8.0.0

## 1.0.2

### Patch Changes

- e8926e920: Minor refactors to improve the type safety of the workspace manifest validation logic in the `readWorkspaceManifest` function.

## 1.0.1

### Patch Changes

- e2a0c7272: Don't fail on an empty `pnpm-workspace.yaml` file [#7307](https://github.com/pnpm/pnpm/issues/7307).

## 1.0.0

### Major Changes

- 3f7e65e10: Initial release.
