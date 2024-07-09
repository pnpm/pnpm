# @pnpm/workspace.read-manifest

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
