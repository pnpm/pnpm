# @pnpm/dependency-path

## 5.1.2

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/types@11.0.0
  - @pnpm/crypto.base32-hash@3.0.0

## 5.1.1

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/crypto.base32-hash@3.0.0

## 5.1.0

### Minor Changes

- 47341e5: **Semi-breaking.** Dependency key names in the lockfile are shortened if they are longer than 1000 characters. We don't expect this change to affect many users. Affected users most probably can't run install successfully at the moment. This change is required to fix some edge cases in which installation fails with an out-of-memory error or "Invalid string length (RangeError: Invalid string length)" error. The max allowed length of the dependency key can be controlled with the `peers-suffix-max-length` setting [#8177](https://github.com/pnpm/pnpm/pull/8177).

### Patch Changes

- @pnpm/crypto.base32-hash@3.0.0

## 5.0.0

### Major Changes

- Breaking changes to the API.

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0
  - @pnpm/crypto.base32-hash@3.0.0

## 4.0.0

### Major Changes

- 9719a42: New setting called `virtual-store-dir-max-length` added to modify the maximum allowed length of the directories inside `node_modules/.pnpm`. The default length is set to 120 characters. This setting is particularly useful on Windows, where there is a limit to the maximum length of a file path [#7355](https://github.com/pnpm/pnpm/issues/7355).

### Patch Changes

- @pnpm/crypto.base32-hash@3.0.0

## 3.0.0

### Major Changes

- cdd8365: Package ID does not contain the registry domain.
- 89b396b: createPeersFolderSuffix renamed to createPeersDirSuffix.
- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.
- d381a60: Support for lockfile v5 is dropped. Use pnpm v8 to convert lockfile v5 to lockfile v6 [#7470](https://github.com/pnpm/pnpm/pull/7470).

### Minor Changes

- 98a1266: createPeersDirSuffix may accept dep path.

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [43cdd87]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0
  - @pnpm/crypto.base32-hash@3.0.0

## 2.1.7

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/types@9.4.2
  - @pnpm/crypto.base32-hash@2.0.0

## 2.1.6

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1
  - @pnpm/crypto.base32-hash@2.0.0

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
