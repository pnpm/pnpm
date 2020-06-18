# @pnpm/plugin-commands-store

## 2.0.12

### Patch Changes

- @pnpm/store-connection-manager@0.3.10

## 2.0.11

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/config@9.2.0
  - @pnpm/get-context@3.0.0
  - @pnpm/cli-utils@0.4.9
  - dependency-path@5.0.1
  - @pnpm/lockfile-utils@2.0.14
  - @pnpm/normalize-registries@1.0.2
  - @pnpm/pick-registry-for-package@1.0.2
  - @pnpm/store-controller-types@8.0.1
  - @pnpm/store-connection-manager@0.3.9
  - @pnpm/cafs@1.0.4

## 2.0.10

### Patch Changes

- Updated dependencies [492805ee3]
  - @pnpm/cafs@1.0.3
  - @pnpm/store-connection-manager@0.3.8

## 2.0.9

### Patch Changes

- Updated dependencies [41d92948b]
- Updated dependencies [e934b1a48]
  - dependency-path@5.0.0
  - @pnpm/cli-utils@0.4.8
  - @pnpm/lockfile-utils@2.0.13
  - @pnpm/store-connection-manager@0.3.7

## 2.0.8

### Patch Changes

- 0e7ec4533: Remove @pnpm/check-package from dependencies.
- d3ddd023c: Update p-limit to v3.
- Updated dependencies [d3ddd023c]
  - @pnpm/cafs@1.0.2
  - @pnpm/store-connection-manager@0.3.6
  - @pnpm/get-context@2.1.2
  - @pnpm/cli-utils@0.4.7

## 2.0.7

### Patch Changes

- @pnpm/store-connection-manager@0.3.5

## 2.0.6

### Patch Changes

- @pnpm/store-connection-manager@0.3.4

## 2.0.5

### Patch Changes

- @pnpm/store-connection-manager@0.3.3

## 2.0.4

### Patch Changes

- Updated dependencies [ffddf34a8]
  - @pnpm/config@9.1.0
  - @pnpm/cli-utils@0.4.6
  - @pnpm/store-connection-manager@0.3.2
  - @pnpm/cafs@1.0.1

## 2.0.3

### Patch Changes

- @pnpm/store-connection-manager@0.3.1

## 2.0.2

### Patch Changes

- Updated dependencies [58c02009f]
  - @pnpm/get-context@2.1.1

## 2.0.1

### Patch Changes

- Updated dependencies [327bfbf02]
  - @pnpm/get-context@2.1.0

## 2.0.0

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- 9596774f2: Store the package index files in the CAFS to reduce directory nesting.
- da091c711: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- 802d145fc: Remove `independent-leaves` support.
- b6a82072e: Using a content-addressable filesystem for storing packages.
- 471149e66: Change the format of the package index file. Move all the files info into a "files" property.
- 9fbb74ecb: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [242cf8737]
- Updated dependencies [9596774f2]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [3f73eaf0c]
- Updated dependencies [f516d266c]
- Updated dependencies [7852deea3]
- Updated dependencies [da091c711]
- Updated dependencies [42e6490d1]
- Updated dependencies [e11019b89]
- Updated dependencies [a5febb913]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [a5febb913]
- Updated dependencies [c207d994f]
- Updated dependencies [45fdcfde2]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [471149e66]
- Updated dependencies [42e6490d1]
- Updated dependencies [e3990787a]
  - @pnpm/config@9.0.0
  - @pnpm/cafs@1.0.0
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/get-context@2.0.0
  - @pnpm/store-connection-manager@0.3.0
  - @pnpm/types@6.0.0
  - @pnpm/cli-utils@0.4.5
  - dependency-path@4.0.7
  - @pnpm/error@1.2.1
  - @pnpm/lockfile-utils@2.0.12
  - @pnpm/normalize-registries@1.0.1
  - @pnpm/parse-wanted-dependency@1.0.1
  - @pnpm/pick-registry-for-package@1.0.1

## 2.0.0-alpha.5

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [242cf8737]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [45fdcfde2]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/config@9.0.0-alpha.2
  - @pnpm/store-controller-types@8.0.0-alpha.4
  - @pnpm/cafs@1.0.0-alpha.5
  - @pnpm/store-connection-manager@0.3.0-alpha.5
  - @pnpm/cli-utils@0.4.5-alpha.2
  - @pnpm/get-context@1.2.2-alpha.2
  - @pnpm/lockfile-utils@2.0.12-alpha.1

## 2.0.0-alpha.4

### Major Changes

- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- 471149e6: Change the format of the package index file. Move all the files info into a "files" property.
- 9fbb74ec: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- Updated dependencies [3f73eaf0]
- Updated dependencies [da091c71]
- Updated dependencies [471149e6]
- Updated dependencies [e3990787]
  - @pnpm/get-context@2.0.0-alpha.1
  - @pnpm/store-connection-manager@0.3.0-alpha.4
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/cafs@1.0.0-alpha.4
  - @pnpm/cli-utils@0.4.5-alpha.1
  - @pnpm/config@8.3.1-alpha.1
  - dependency-path@4.0.7-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.0
  - @pnpm/normalize-registries@1.0.1-alpha.0
  - @pnpm/pick-registry-for-package@1.0.1-alpha.0

## 2.0.0-alpha.3

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- 9596774f2: Store the package index files in the CAFS to reduce directory nesting.

### Patch Changes

- Updated dependencies [9596774f2]
- Updated dependencies [7852deea3]
  - @pnpm/cafs@1.0.0-alpha.3
  - @pnpm/config@8.3.1-alpha.0
  - @pnpm/get-context@1.2.2-alpha.0
  - @pnpm/store-connection-manager@0.2.32-alpha.3
  - @pnpm/cli-utils@0.4.5-alpha.0

## 1.0.11-alpha.2

### Patch Changes

- Updated dependencies [42e6490d1]
  - @pnpm/store-controller-types@8.0.0-alpha.2
  - @pnpm/store-connection-manager@0.2.32-alpha.2

## 1.0.11-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/store-controller-types@8.0.0-alpha.1
  - @pnpm/store-connection-manager@0.2.32-alpha.1

## 2.0.0-alpha.0

### Major Changes

- 91c4b5954: Using a content-addressable filesystem for storing packages.

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/store-controller-types@8.0.0-alpha.0
  - @pnpm/store-connection-manager@0.3.0-alpha.0

## 1.0.10

### Patch Changes

- 907c63a48: Update `@pnpm/store-path`.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/store-connection-manager@0.2.31
  - @pnpm/get-context@1.2.1
  - @pnpm/cli-utils@0.4.4
