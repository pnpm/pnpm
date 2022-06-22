# @pnpm/lockfile-types

## 4.1.0

### Minor Changes

- 2a34b21ce: Dependencies patching is possible via the `pnpm.patchedDependencies` field of the `package.json`.
  To patch a package, the package name, exact version, and the relative path to the patch file should be specified. For instance:

  ```json
  {
    "pnpm": {
      "patchedDependencies": {
        "eslint@1.0.0": "./patches/eslint@1.0.0.patch"
      }
    }
  }
  ```

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0

## 4.0.3

### Patch Changes

- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0

## 4.0.2

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0

## 4.0.1

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1

## 4.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0

## 3.2.0

### Minor Changes

- b138d048c: New optional field supported: `onlyBuiltDependencies`.

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0

## 3.1.5

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0

## 3.1.4

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0

## 3.1.3

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1

## 3.1.2

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0

## 3.1.1

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0

## 3.1.0

### Minor Changes

- 4ab87844a: New optional property added to project snapshots: `dependenciesMeta`.

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0

## 3.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Minor Changes

- 6871d74b2: Add new transitivePeerDependencies field to lockfile.

## 2.2.0

### Minor Changes

- 9ad8c27bf: Add optional neverBuiltDependencies property to the lockfile object.

## 2.1.1

### Patch Changes

- b5d694e7f: Use pnpm.overrides instead of resolutions. Still support resolutions for partial compatibility with Yarn and for avoiding a breaking change.

## 2.1.0

### Minor Changes

- d54043ee4: A new optional field added to the lockfile type: resolutions.

## 2.0.1

### Patch Changes

- 6a8a97eee: Fix the type of bundledDependencies field.

## 2.0.1-alpha.0

### Patch Changes

- 6a8a97eee: Fix the type of bundledDependencies field.
