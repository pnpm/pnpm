# @pnpm/modules-cleaner

## 10.0.16

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/store-controller-types@9.2.0
  - @pnpm/lockfile-utils@2.0.20
  - @pnpm/filter-lockfile@4.0.14

## 10.0.15

### Patch Changes

- @pnpm/filter-lockfile@4.0.13
- @pnpm/remove-bins@1.0.9

## 10.0.14

### Patch Changes

- Updated dependencies [39142e2ad]
  - dependency-path@5.0.6
  - @pnpm/filter-lockfile@4.0.12
  - @pnpm/lockfile-utils@2.0.19

## 10.0.13

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/lockfile-types@2.1.1
  - @pnpm/types@6.3.1
  - @pnpm/filter-lockfile@4.0.11
  - @pnpm/lockfile-utils@2.0.18
  - @pnpm/core-loggers@5.0.2
  - dependency-path@5.0.5
  - @pnpm/remove-bins@1.0.8
  - @pnpm/store-controller-types@9.1.2

## 10.0.12

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [d54043ee4]
  - @pnpm/lockfile-types@2.1.0
  - @pnpm/types@6.3.0
  - @pnpm/filter-lockfile@4.0.10
  - @pnpm/lockfile-utils@2.0.17
  - @pnpm/core-loggers@5.0.1
  - dependency-path@5.0.4
  - @pnpm/remove-bins@1.0.7
  - @pnpm/store-controller-types@9.1.1

## 10.0.11

### Patch Changes

- Updated dependencies [0a6544043]
  - @pnpm/store-controller-types@9.1.0

## 10.0.10

### Patch Changes

- Updated dependencies [86cd72de3]
- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0
  - @pnpm/store-controller-types@9.0.0
  - @pnpm/remove-bins@1.0.6
  - @pnpm/filter-lockfile@4.0.9

## 10.0.9

### Patch Changes

- @pnpm/filter-lockfile@4.0.8
- @pnpm/remove-bins@1.0.5

## 10.0.8

### Patch Changes

- @pnpm/remove-bins@1.0.4

## 10.0.7

### Patch Changes

- @pnpm/filter-lockfile@4.0.7

## 10.0.6

### Patch Changes

- Updated dependencies [24af41f20]
  - @pnpm/read-modules-dir@2.0.3

## 10.0.5

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [1140ef721]
- Updated dependencies [a2ef8084f]
  - @pnpm/lockfile-utils@2.0.16
  - dependency-path@5.0.3
  - @pnpm/filter-lockfile@4.0.6
  - @pnpm/read-modules-dir@2.0.2
  - @pnpm/remove-bins@1.0.3

## 10.0.4

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0
  - @pnpm/remove-bins@1.0.2
  - @pnpm/filter-lockfile@4.0.5

## 10.0.3

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/core-loggers@4.1.2
  - dependency-path@5.0.2
  - @pnpm/filter-lockfile@4.0.4
  - @pnpm/lockfile-utils@2.0.15
  - @pnpm/remove-bins@1.0.1
  - @pnpm/store-controller-types@8.0.2

## 10.0.2

### Patch Changes

- 57d08f303: Remove global bins when unlinking.
- Updated dependencies [57d08f303]
  - @pnpm/remove-bins@1.0.0

## 10.0.1

### Patch Changes

- Updated dependencies [1520e3d6f]
  - @pnpm/package-bins@4.0.6

## 10.0.0

### Major Changes

- 71a8c8ce3: Replaced `hoistedAliases` with `hoistedDependencies`.

  Added `publicHoistedModulesDir` option.

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/core-loggers@4.1.1
  - dependency-path@5.0.1
  - @pnpm/filter-lockfile@4.0.3
  - @pnpm/lockfile-utils@2.0.14
  - @pnpm/package-bins@4.0.5
  - @pnpm/read-package-json@3.1.2
  - @pnpm/store-controller-types@8.0.1

## 9.0.2

### Patch Changes

- Updated dependencies [41d92948b]
  - dependency-path@5.0.0
  - @pnpm/filter-lockfile@4.0.2
  - @pnpm/lockfile-utils@2.0.13

## 9.0.1

### Patch Changes

- Updated dependencies [2ebb7af33]
  - @pnpm/core-loggers@4.1.0
  - @pnpm/filter-lockfile@4.0.1

## 9.0.0

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- da091c711: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- 9fbb74ecb: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Minor Changes

- 7179cc560: Don't try to remove empty branches of a directory tree, when pruning `node_modules`.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [c25cccdad]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [f516d266c]
- Updated dependencies [da091c711]
- Updated dependencies [42e6490d1]
- Updated dependencies [2485eaf60]
- Updated dependencies [a5febb913]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [6a8a97eee]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/filter-lockfile@4.0.0
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/types@6.0.0
  - @pnpm/lockfile-types@2.0.1
  - @pnpm/core-loggers@4.0.2
  - dependency-path@4.0.7
  - @pnpm/lockfile-utils@2.0.12
  - @pnpm/package-bins@4.0.4
  - @pnpm/read-modules-dir@2.0.2
  - @pnpm/read-package-json@3.1.1

## 9.0.0-alpha.5

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [c25cccdad]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [2485eaf60]
- Updated dependencies [a5febb913]
- Updated dependencies [6a8a97eee]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/filter-lockfile@4.0.0-alpha.2
  - @pnpm/store-controller-types@8.0.0-alpha.4
  - @pnpm/lockfile-types@2.0.1-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.1

## 9.0.0-alpha.4

### Major Changes

- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- 9fbb74ec: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Minor Changes

- 7179cc56: Don't try to remove empty branches of a directory tree, when pruning `node_modules`.

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/core-loggers@4.0.2-alpha.0
  - dependency-path@4.0.7-alpha.0
  - @pnpm/filter-lockfile@3.2.3-alpha.1
  - @pnpm/lockfile-utils@2.0.12-alpha.0
  - @pnpm/package-bins@4.0.4-alpha.0
  - @pnpm/read-package-json@3.1.1-alpha.0

## 9.0.0-alpha.3

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.

### Patch Changes

- @pnpm/filter-lockfile@3.2.3-alpha.0

## 8.0.17-alpha.2

### Patch Changes

- Updated dependencies [42e6490d1]
  - @pnpm/store-controller-types@8.0.0-alpha.2

## 8.0.17-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/store-controller-types@8.0.0-alpha.1

## 8.0.17-alpha.0

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/store-controller-types@8.0.0-alpha.0

## 8.0.16

### Patch Changes

- Updated dependencies [907c63a48]
  - @pnpm/filter-lockfile@3.2.2
  - @pnpm/lockfile-utils@2.0.11
