# @pnpm/manifest-utils

## 3.1.0

### Minor Changes

- f5621a42c: A new value `rolling` for option `save-workspace-protocol`. When selected, pnpm will save workspace versions using a rolling alias (e.g. `"foo": "workspace:^"`) instead of pinning the current version number (e.g. `"foo": "workspace:^1.0.0"`). Usage example:

  ```
  pnpm --save-workspace-protocol=rolling add foo
  ```

## 3.0.6

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/core-loggers@7.0.5

## 3.0.5

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/core-loggers@7.0.4

## 3.0.4

### Patch Changes

- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0
  - @pnpm/core-loggers@7.0.3

## 3.0.3

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/core-loggers@7.0.2

## 3.0.2

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/core-loggers@7.0.1

## 3.0.1

### Patch Changes

- 618842b0d: It should be possible to reference a workspace project that has no version specified in its `package.json` [#4487](https://github.com/pnpm/pnpm/pull/4487).
  - @pnpm/error@3.0.1

## 3.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/core-loggers@7.0.0
  - @pnpm/error@3.0.0

## 2.1.9

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0

## 2.1.8

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/core-loggers@6.1.4

## 2.1.7

### Patch Changes

- 8a2cad034: fix: version set be correct when set empty version

## 2.1.6

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/core-loggers@6.1.3

## 2.1.5

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/core-loggers@6.1.2

## 2.1.4

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/core-loggers@6.1.1

## 2.1.3

### Patch Changes

- Updated dependencies [ba9b2eba1]
- Updated dependencies [ba9b2eba1]
  - @pnpm/core-loggers@6.1.0
  - @pnpm/types@7.7.0

## 2.1.2

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/core-loggers@6.0.6

## 2.1.1

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/core-loggers@6.0.5

## 2.1.0

### Minor Changes

- 553a5d840: The path to Node.js executable is added to `dependenciesMeta` when `nodeExecPath` is specified in the`PackageSpecObject`.

## 2.0.4

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/core-loggers@6.0.4

## 2.0.3

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/core-loggers@6.0.3

## 2.0.2

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/core-loggers@6.0.2

## 2.0.1

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0
  - @pnpm/core-loggers@6.0.1

## 2.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
- Updated dependencies [90487a3a8]
  - @pnpm/core-loggers@6.0.0
  - @pnpm/error@2.0.0
  - @pnpm/types@7.0.0

## 1.1.5

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0
  - @pnpm/core-loggers@5.0.3

## 1.1.4

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0

## 1.1.3

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/core-loggers@5.0.2

## 1.1.2

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/core-loggers@5.0.1

## 1.1.1

### Patch Changes

- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0

## 1.1.0

### Minor Changes

- e2f6b40b1: `getSpecFromPackageManifest()` added.
- e2f6b40b1: `getPref()` added.
- e2f6b40b1: `updateProjectManifestObject()` added.

## 1.0.3

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0

## 1.0.2

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0

## 1.0.1

### Patch Changes

- Updated dependencies [da091c711]
  - @pnpm/types@6.0.0

## 1.0.1-alpha.0

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
