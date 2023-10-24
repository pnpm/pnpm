# @pnpm/dependency-path

## 2.1.5

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0
  - @pnpm/crypto.base32-hash@2.0.0

## 2.1.4

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/crypto.base32-hash@2.0.0

## 2.1.3

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/crypto.base32-hash@2.0.0

## 2.1.2

### Patch Changes

- Updated dependencies [a9e0b7cbf]
  - @pnpm/types@9.1.0
  - @pnpm/crypto.base32-hash@2.0.0

## 2.1.1

### Patch Changes

- c0760128d: bump semver to 7.4.0

## 2.1.0

### Minor Changes

- 94f94eed6: Export `indexOfPeersSuffix`.

### Patch Changes

- 5087636b6: Repeat installation should work on a project that has a dependency with () chars in the scope name [#6348](https://github.com/pnpm/pnpm/issues/6348).

## 2.0.0

### Major Changes

- ca8f51e60: Change the way depPathToFilename is making paths shorter.
- eceaa8b8b: Node.js 14 support dropped.
- 0e26acb0f: Rename createPeersFolderSuffixNewFormat to createPeersFolderSuffix.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/crypto.base32-hash@2.0.0
  - @pnpm/types@9.0.0

## 1.1.3

### Patch Changes

- d89d7a078: `parse()` should not fail on dependency path pointing to a local dependency.

## 1.1.2

### Patch Changes

- 9247f6781: Directories inside the virtual store should not contain the ( or ) chars. This is to fix issues with storybook and the new v6 `pnpm-lock.yaml` lockfile format [#5976](https://github.com/pnpm/pnpm/issues/5976).

## 1.1.1

### Patch Changes

- 0f6e95872: The new lockfile format should not be broken on repeat install.

## 1.1.0

### Minor Changes

- 3ebce5db7: Updated the functions to support dependency paths used in the 6th version of the lockfile. Exported a new function: createPeersFolderSuffixNewFormat.

### Patch Changes

- @pnpm/crypto.base32-hash@1.0.1

## 1.0.1

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0
  - @pnpm/crypto.base32-hash@1.0.1

## 1.0.0

### Major Changes

- 313702d76: Project renamed from `dependency-path` to `@pnpm/dependency-path`.
