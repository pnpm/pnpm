# @pnpm/workspace.read-manifest

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Minor Changes

- 075aa99: Add support for a global YAML config file named `config.yaml`.

  Now configurations are divided into 2 categories:

  - Registry and auth settings which can be stored in INI files such as global `rc` and local `.npmrc`.
  - pnpm-specific settings which can only be loaded from YAML files such as global `config.yaml` and local `pnpm-workspace.yaml`.

- 2b14c74: The `validateWorkspaceManifest` function is now exported and can be used to validate whether a workspace manifest object's schema is correct.

### Patch Changes

- Updated dependencies [c55c614]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [491a84f]
- Updated dependencies [075aa99]
- Updated dependencies [7d2fd48]
- Updated dependencies [efb48dc]
- Updated dependencies [50fbeca]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
  - @pnpm/constants@1002.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/error@1001.0.0

## 1000.2.5

### Patch Changes

- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
  - @pnpm/types@1000.9.0

## 1000.2.4

### Patch Changes

- Updated dependencies [6365bc4]
  - @pnpm/constants@1001.3.1
  - @pnpm/error@1000.0.5

## 1000.2.3

### Patch Changes

- Updated dependencies [e792927]
  - @pnpm/types@1000.8.0

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
