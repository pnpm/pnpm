# @pnpm/lockfile-types

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
