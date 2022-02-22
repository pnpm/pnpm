# @pnpm/exportable-manifest

## 2.3.1

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/read-project-manifest@2.0.12

## 2.3.0

### Minor Changes

- e1b459008: Remove meaningless keys from `publishConfig` when the `pack` or `publish` commands are used [#4311](https://github.com/pnpm/pnpm/issues/4311)

## 2.2.4

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/read-project-manifest@2.0.11

## 2.2.3

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/read-project-manifest@2.0.10

## 2.2.2

### Patch Changes

- 6493e0c93: add readme file to published package.json file
- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/read-project-manifest@2.0.9

## 2.2.1

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - @pnpm/read-project-manifest@2.0.8

## 2.2.0

### Minor Changes

- 6428690e2: Allow to set `os` and `cpu` in `publishConfig`.

## 2.1.8

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/read-project-manifest@2.0.7

## 2.1.7

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/read-project-manifest@2.0.6

## 2.1.6

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/read-project-manifest@2.0.5

## 2.1.5

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/read-project-manifest@2.0.4

## 2.1.4

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/read-project-manifest@2.0.3

## 2.1.3

### Patch Changes

- a1a03d145: Import only the required functions from ramda.

## 2.1.2

### Patch Changes

- 6a1468495: Adds support for `type` and `imports` in publishConfig

## 2.1.1

### Patch Changes

- @pnpm/read-project-manifest@2.0.2

## 2.1.0

### Minor Changes

- 85fb21a83: Add support for workspace:^ and workspace:~ aliases

### Patch Changes

- Updated dependencies [6e9c112af]
- Updated dependencies [97c64bae4]
  - @pnpm/read-project-manifest@2.0.1
  - @pnpm/types@7.1.0

## 2.0.1

### Patch Changes

- 561276d2c: Remove publish lifecycle events from manifest to avoid npm running them.

## 2.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
  - @pnpm/error@2.0.0
  - @pnpm/read-project-manifest@2.0.0
  - @pnpm/types@7.0.0

## 1.2.2

### Patch Changes

- Updated dependencies [ad113645b]
  - @pnpm/read-project-manifest@1.1.7

## 1.2.1

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0
  - @pnpm/read-project-manifest@1.1.6

## 1.2.0

### Minor Changes

- c854f8547: Remove the "pnpm" property that stores pnpm settings from the manifest.

## 1.1.0

### Minor Changes

- 284e95c5e: Convert relative workspace paths to version specs.
- 084614f55: Support aliases to workspace packages. For instance, `"foo": "workspace:bar@*"` will link bar from the repository but aliased to foo. Before publish, these specs are converted to regular aliased versions.

## 1.0.8

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/read-project-manifest@1.1.5

## 1.0.7

### Patch Changes

- @pnpm/read-project-manifest@1.1.4

## 1.0.6

### Patch Changes

- @pnpm/read-project-manifest@1.1.3

## 1.0.5

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/read-project-manifest@1.1.2

## 1.0.4

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/read-project-manifest@1.1.1

## 1.0.3

### Patch Changes

- Updated dependencies [2762781cc]
  - @pnpm/read-project-manifest@1.1.0

## 1.0.2

### Patch Changes

- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1
  - @pnpm/read-project-manifest@1.0.13

## 1.0.1

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/read-project-manifest@1.0.12

## 1.0.0

### Major Changes

- edf1f412e: Package created.
