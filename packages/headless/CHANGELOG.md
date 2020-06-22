# @pnpm/headless

## 14.0.5

### Patch Changes

- @pnpm/package-requester@12.0.6

## 14.0.4

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/build-modules@5.0.6
  - @pnpm/core-loggers@4.1.2
  - dependency-path@5.0.2
  - @pnpm/filter-lockfile@4.0.4
  - @pnpm/hoist@4.0.3
  - @pnpm/lifecycle@9.1.3
  - @pnpm/link-bins@5.3.7
  - @pnpm/lockfile-file@3.0.11
  - @pnpm/lockfile-utils@2.0.15
  - @pnpm/modules-cleaner@10.0.3
  - @pnpm/modules-yaml@8.0.1
  - @pnpm/package-requester@12.0.6
  - @pnpm/read-package-json@3.1.3
  - @pnpm/read-project-manifest@1.0.9
  - @pnpm/store-controller-types@8.0.2
  - @pnpm/symlink-dependency@3.0.8

## 14.0.3

### Patch Changes

- Updated dependencies [57d08f303]
  - @pnpm/modules-cleaner@10.0.2

## 14.0.2

### Patch Changes

- @pnpm/package-requester@12.0.5
- @pnpm/link-bins@5.3.6
- @pnpm/modules-cleaner@10.0.1
- @pnpm/build-modules@5.0.5
- @pnpm/hoist@4.0.2

## 14.0.1

### Patch Changes

- Updated dependencies [0a2f3ecc6]
  - @pnpm/hoist@4.0.1

## 14.0.0

### Major Changes

- 71a8c8ce3: `hoistedAliases` replaced with `hoistedDependencies`.

  `shamefullyHoist` replaced with `publicHoistPattern`.

- 71a8c8ce3: Breaking changes to the `node_modules/.modules.yaml` file:
  - `hoistedAliases` replaced with `hoistedDependencies`.
  - `shamefullyHoist` replaced with `publicHoistPattern`.

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [e1ca9fc13]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/hoist@4.0.0
  - @pnpm/link-bins@5.3.5
  - @pnpm/modules-cleaner@10.0.0
  - @pnpm/modules-yaml@8.0.0
  - @pnpm/build-modules@5.0.4
  - @pnpm/core-loggers@4.1.1
  - dependency-path@5.0.1
  - @pnpm/filter-lockfile@4.0.3
  - @pnpm/lifecycle@9.1.2
  - @pnpm/lockfile-file@3.0.10
  - @pnpm/lockfile-utils@2.0.14
  - @pnpm/package-requester@12.0.5
  - @pnpm/read-package-json@3.1.2
  - @pnpm/read-project-manifest@1.0.8
  - @pnpm/store-controller-types@8.0.1
  - @pnpm/symlink-dependency@3.0.7

## 13.0.6

### Patch Changes

- @pnpm/package-requester@12.0.4

## 13.0.5

### Patch Changes

- Updated dependencies [41d92948b]
- Updated dependencies [57c510f00]
  - dependency-path@5.0.0
  - @pnpm/read-project-manifest@1.0.7
  - @pnpm/filter-lockfile@4.0.2
  - @pnpm/hoist@3.0.2
  - @pnpm/lockfile-utils@2.0.13
  - @pnpm/modules-cleaner@9.0.2
  - @pnpm/link-bins@5.3.4
  - @pnpm/build-modules@5.0.3
  - @pnpm/package-requester@12.0.3

## 13.0.4

### Patch Changes

- d3ddd023c: Update p-limit to v3.
- Updated dependencies [d3ddd023c]
- Updated dependencies [2ebb7af33]
- Updated dependencies [68d8dc68f]
  - @pnpm/build-modules@5.0.2
  - @pnpm/lifecycle@9.1.1
  - @pnpm/package-requester@12.0.3
  - @pnpm/core-loggers@4.1.0
  - @pnpm/modules-cleaner@9.0.1
  - @pnpm/symlink-dependency@3.0.6
  - @pnpm/filter-lockfile@4.0.1
  - @pnpm/hoist@3.0.1

## 13.0.3

### Patch Changes

- Updated dependencies [a203bc138]
  - @pnpm/package-requester@12.0.2

## 13.0.2

### Patch Changes

- @pnpm/package-requester@12.0.1

## 13.0.1

### Patch Changes

- Updated dependencies [8094b2a62]
  - @pnpm/lifecycle@9.1.0
  - @pnpm/package-requester@12.0.1
  - @pnpm/build-modules@5.0.1

## 13.0.0

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- 3f73eaf0c: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.
- 802d145fc: Remove `independent-leaves` support.
- b6a82072e: Using a content-addressable filesystem for storing packages.
- a5febb913: The importPackage function of the store controller is importing packages directly from the side-effects cache.
- 9fbb74ecb: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [b5f66c0f2]
- Updated dependencies [ca9f50844]
- Updated dependencies [9596774f2]
- Updated dependencies [7179cc560]
- Updated dependencies [c25cccdad]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [3f73eaf0c]
- Updated dependencies [f516d266c]
- Updated dependencies [da091c711]
- Updated dependencies [9b1b520d9]
- Updated dependencies [f35a3ec1c]
- Updated dependencies [a7d20d927]
- Updated dependencies [42e6490d1]
- Updated dependencies [2485eaf60]
- Updated dependencies [64bae33c4]
- Updated dependencies [a5febb913]
- Updated dependencies [bb59db642]
- Updated dependencies [802d145fc]
- Updated dependencies [f93583d52]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [a5febb913]
- Updated dependencies [c207d994f]
- Updated dependencies [a5febb913]
- Updated dependencies [4f5801b1c]
- Updated dependencies [a5febb913]
- Updated dependencies [471149e66]
- Updated dependencies [42e6490d1]
- Updated dependencies [9fbb74ecb]
- Updated dependencies [e3990787a]
  - @pnpm/constants@4.0.0
  - @pnpm/hoist@3.0.0
  - @pnpm/modules-cleaner@9.0.0
  - @pnpm/package-requester@12.0.0
  - @pnpm/filter-lockfile@4.0.0
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/modules-yaml@7.0.0
  - @pnpm/types@6.0.0
  - @pnpm/build-modules@5.0.0
  - @pnpm/lifecycle@9.0.0
  - @pnpm/core-loggers@4.0.2
  - dependency-path@4.0.7
  - @pnpm/error@1.2.1
  - @pnpm/link-bins@5.3.3
  - @pnpm/lockfile-file@3.0.9
  - @pnpm/lockfile-utils@2.0.12
  - @pnpm/matcher@1.0.3
  - @pnpm/read-package-json@3.1.1
  - @pnpm/read-project-manifest@1.0.6
  - @pnpm/symlink-dependency@3.0.5

## 13.0.0-alpha.5

### Major Changes

- a5febb913: The importPackage function of the store controller is importing packages directly from the side-effects cache.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [ca9f50844]
- Updated dependencies [c25cccdad]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [a7d20d927]
- Updated dependencies [2485eaf60]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/filter-lockfile@4.0.0-alpha.2
  - @pnpm/package-requester@12.0.0-alpha.5
  - @pnpm/store-controller-types@8.0.0-alpha.4
  - @pnpm/hoist@3.0.0-alpha.2
  - @pnpm/modules-cleaner@9.0.0-alpha.5
  - @pnpm/build-modules@5.0.0-alpha.5
  - @pnpm/lockfile-file@3.0.9-alpha.2
  - @pnpm/lockfile-utils@2.0.12-alpha.1

## 13.0.0-alpha.4

### Major Changes

- 3f73eaf0: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.
- 9fbb74ec: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- Updated dependencies [7179cc56]
- Updated dependencies [3f73eaf0]
- Updated dependencies [da091c71]
- Updated dependencies [471149e6]
- Updated dependencies [9fbb74ec]
- Updated dependencies [e3990787]
  - @pnpm/modules-cleaner@9.0.0-alpha.4
  - @pnpm/modules-yaml@7.0.0-alpha.0
  - @pnpm/package-requester@12.0.0-alpha.4
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/hoist@3.0.0-alpha.1
  - @pnpm/build-modules@5.0.0-alpha.4
  - @pnpm/lifecycle@9.0.0-alpha.1
  - @pnpm/core-loggers@4.0.2-alpha.0
  - dependency-path@4.0.7-alpha.0
  - @pnpm/filter-lockfile@3.2.3-alpha.1
  - @pnpm/link-bins@5.3.3-alpha.0
  - @pnpm/lockfile-file@3.0.9-alpha.1
  - @pnpm/lockfile-utils@2.0.12-alpha.0
  - @pnpm/read-package-json@3.1.1-alpha.0
  - @pnpm/read-project-manifest@1.0.6-alpha.0
  - @pnpm/symlink-dependency@3.0.5-alpha.0

## 13.0.0-alpha.3

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [9596774f2]
  - @pnpm/constants@4.0.0-alpha.0
  - @pnpm/hoist@3.0.0-alpha.0
  - @pnpm/modules-cleaner@9.0.0-alpha.3
  - @pnpm/package-requester@12.0.0-alpha.3
  - @pnpm/build-modules@4.1.15-alpha.3
  - @pnpm/filter-lockfile@3.2.3-alpha.0
  - @pnpm/lockfile-file@3.0.9-alpha.0

## 12.2.2-alpha.2

### Patch Changes

- Updated dependencies [f35a3ec1c]
- Updated dependencies [42e6490d1]
- Updated dependencies [64bae33c4]
- Updated dependencies [c207d994f]
- Updated dependencies [42e6490d1]
  - @pnpm/lifecycle@8.2.0-alpha.0
  - @pnpm/package-requester@12.0.0-alpha.2
  - @pnpm/store-controller-types@8.0.0-alpha.2
  - @pnpm/build-modules@4.1.14-alpha.2
  - @pnpm/modules-cleaner@8.0.17-alpha.2

## 12.2.2-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
- Updated dependencies [f93583d52]
  - @pnpm/package-requester@12.0.0-alpha.1
  - @pnpm/store-controller-types@8.0.0-alpha.1
  - @pnpm/build-modules@4.1.14-alpha.1
  - @pnpm/modules-cleaner@8.0.17-alpha.1

## 13.0.0-alpha.0

### Major Changes

- 91c4b5954: Using a content-addressable filesystem for storing packages.

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/package-requester@12.0.0-alpha.0
  - @pnpm/store-controller-types@8.0.0-alpha.0
  - @pnpm/build-modules@4.1.14-alpha.0
  - @pnpm/modules-cleaner@8.0.17-alpha.0

## 12.2.2

### Patch Changes

- Updated dependencies [2ec4c4eb9]
  - @pnpm/lifecycle@8.2.0
  - @pnpm/build-modules@4.1.14

## 12.2.1

### Patch Changes

- 907c63a48: Update `@pnpm/store-path`.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/package-requester@11.0.6
  - @pnpm/symlink-dependency@3.0.4
  - @pnpm/link-bins@5.3.2
  - @pnpm/lockfile-file@3.0.8
  - @pnpm/matcher@1.0.2
  - @pnpm/filter-lockfile@3.2.2
  - @pnpm/lockfile-utils@2.0.11
  - @pnpm/modules-yaml@6.0.2
  - @pnpm/hoist@2.2.3
  - @pnpm/build-modules@4.1.13
  - @pnpm/modules-cleaner@8.0.16
  - @pnpm/read-project-manifest@1.0.5
