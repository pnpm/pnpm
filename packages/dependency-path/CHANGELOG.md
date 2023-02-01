# @pnpm/dependency-path

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
