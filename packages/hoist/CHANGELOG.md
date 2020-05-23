# @pnpm/hoist

## 3.0.0

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- 802d145fc: Remove `independent-leaves` support.
- 9fbb74ecb: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [b5f66c0f2]
- Updated dependencies [ca9f50844]
- Updated dependencies [142f8caf7]
- Updated dependencies [da091c711]
- Updated dependencies [6a8a97eee]
- Updated dependencies [4f5801b1c]
  - @pnpm/constants@4.0.0
  - @pnpm/lockfile-walker@3.0.0
  - @pnpm/types@6.0.0
  - @pnpm/lockfile-types@2.0.1
  - dependency-path@4.0.7
  - @pnpm/link-bins@5.3.3
  - @pnpm/lockfile-utils@2.0.12
  - @pnpm/symlink-dependency@3.0.5

## 3.0.0-alpha.2

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [ca9f50844]
- Updated dependencies [6a8a97eee]
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/lockfile-types@2.0.1-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.1
  - @pnpm/lockfile-walker@2.0.3-alpha.1

## 3.0.0-alpha.1

### Major Changes

- 9fbb74ec: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - dependency-path@4.0.7-alpha.0
  - @pnpm/link-bins@5.3.3-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.0
  - @pnpm/lockfile-walker@2.0.3-alpha.0
  - @pnpm/symlink-dependency@3.0.5-alpha.0

## 3.0.0-alpha.0

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0

## 2.2.3

### Patch Changes

- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/symlink-dependency@3.0.4
  - @pnpm/link-bins@5.3.2
  - @pnpm/lockfile-utils@2.0.11
