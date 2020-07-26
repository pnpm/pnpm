# @pnpm/resolve-dependencies

## 16.1.0

### Minor Changes

- 8c1cf25b7: New option added: updateMatching. updateMatching is a function that accepts a package name. It returns `true` if the specified package should be updated.

## 16.0.6

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0
  - @pnpm/package-is-installable@4.0.12
  - @pnpm/npm-resolver@9.0.1

## 16.0.5

### Patch Changes

- Updated dependencies [379cdcaf8]
  - @pnpm/npm-resolver@9.0.1

## 16.0.4

### Patch Changes

- 7f25dad04: Only add packages to the skipped set, when they are seen the first time.

## 16.0.3

### Patch Changes

- Updated dependencies [71aeb9a38]
  - @pnpm/npm-resolver@9.0.0

## 16.0.2

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/core-loggers@4.1.2
  - dependency-path@5.0.2
  - @pnpm/lockfile-utils@2.0.15
  - @pnpm/npm-resolver@8.1.2
  - @pnpm/package-is-installable@4.0.11
  - @pnpm/pick-registry-for-package@1.0.3
  - @pnpm/resolver-base@7.0.3
  - @pnpm/store-controller-types@8.0.2

## 16.0.1

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/core-loggers@4.1.1
  - dependency-path@5.0.1
  - @pnpm/lockfile-utils@2.0.14
  - @pnpm/npm-resolver@8.1.1
  - @pnpm/package-is-installable@4.0.10
  - @pnpm/pick-registry-for-package@1.0.2
  - @pnpm/resolver-base@7.0.2
  - @pnpm/store-controller-types@8.0.1

## 16.0.0

### Major Changes

- 41d92948b: Expects direct tarball IDs to start with @.

### Patch Changes

- Updated dependencies [41d92948b]
  - dependency-path@5.0.0
  - @pnpm/lockfile-utils@2.0.13

## 15.1.2

### Patch Changes

- Updated dependencies [4cf7ef367]
- Updated dependencies [d3ddd023c]
- Updated dependencies [2ebb7af33]
  - @pnpm/npm-resolver@8.1.0
  - @pnpm/core-loggers@4.1.0
  - @pnpm/package-is-installable@4.0.9

## 15.1.1

### Patch Changes

- @pnpm/npm-resolver@8.0.1

## 15.1.0

### Minor Changes

- 71b0cb8fd: A new option added: `forceFullResolution`. When `true`, the whole dependency graph will be walked through during resolution.

## 15.0.1

### Patch Changes

- e2c4fdad5: Don't remove resolved peer dependencies from dependencies when lockfile is partially up-to-date.

## 15.0.0

### Major Changes

- 0730bb938: Check the existense of a dependency in `node_modules` at the right location.
- 242cf8737: The `alwaysTryWorkspacePackages` option is removed. A new option called `linkWorkspacePackagesDepth` is added.
  When `linkWorkspacePackageDepth` is `0`, workspace packages are linked to direct dependencies even if these direct
  dependencies are not using workspace ranges (so this is similar to the old `alwaysTryWorkspacePackages=true`).
  `linkWorkspacePackageDepth` also allows to link workspace packages to subdependencies by setting the max depth.
  Setting it to `Infinity` will make the resolution algorithm always prefer packages from the workspace over packages
  from the registry.
- cc8a3bd31: `updateLockfile` options property is removed. `updateDepth=Infinity` should be used instead. Which is set for each project separately.
- 16d1ac0fd: `engineCache` is removed from `ResolvedPackage`. `sideEffectsCache` removed from input options.
- e11019b89: Deprecate the resolution strategy setting. The fewer dependencies strategy is used always.
- 802d145fc: Remove `independent-leaves` support.
- 9fbb74ecb: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Minor Changes

- a5febb913: Package request response contains the path to the files index file.
- b47f9737a: When direct dependencies are present, subdependencies are not reanalyzed on repeat install.

### Patch Changes

- 77bc9b510: Resolve subdependencies only after all parent dependencies were resolved.
- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- 4cc0ead24: Update replace-string to v3.1.0.
- c25cccdad: The lockfile should be recreated correctly when an up-to-date `node_modules` is present.
  The recreated lockfile should contain all the skipped optional dependencies.
- Updated dependencies [5bc033c43]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [f516d266c]
- Updated dependencies [da091c711]
- Updated dependencies [42e6490d1]
- Updated dependencies [a5febb913]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [6a8a97eee]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [f453a5f46]
  - @pnpm/npm-resolver@8.0.0
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/types@6.0.0
  - @pnpm/lockfile-types@2.0.1
  - @pnpm/core-loggers@4.0.2
  - dependency-path@4.0.7
  - @pnpm/error@1.2.1
  - @pnpm/lockfile-utils@2.0.12
  - @pnpm/package-is-installable@4.0.8
  - @pnpm/pick-registry-for-package@1.0.1
  - @pnpm/resolver-base@7.0.1

## 15.0.0-alpha.6

### Major Changes

- 242cf8737: The `alwaysTryWorkspacePackages` option is removed. A new option called `linkWorkspacePackagesDepth` is added.
  When `linkWorkspacePackageDepth` is `0`, workspace packages are linked to direct dependencies even if these direct
  dependencies are not using workspace ranges (so this is similar to the old `alwaysTryWorkspacePackages=true`).
  `linkWorkspacePackageDepth` also allows to link workspace packages to subdependencies by setting the max depth.
  Setting it to `Infinity` will make the resolution algorithm always prefer packages from the workspace over packages
  from the registry.
- cc8a3bd31: `updateLockfile` options property is removed. `updateDepth=Infinity` should be used instead. Which is set for each project separately.
- 16d1ac0fd: `engineCache` is removed from `ResolvedPackage`. `sideEffectsCache` removed from input options.

### Minor Changes

- a5febb913: Package request response contains the path to the files index file.
- b47f9737a: When direct dependencies are present, subdependencies are not reanalyzed on repeat install.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- c25cccdad: The lockfile should be recreated correctly when an up-to-date `node_modules` is present.
  The recreated lockfile should contain all the skipped optional dependencies.
- Updated dependencies [16d1ac0fd]
- Updated dependencies [a5febb913]
- Updated dependencies [6a8a97eee]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/store-controller-types@8.0.0-alpha.4
  - @pnpm/lockfile-types@2.0.1-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.1

## 15.0.0-alpha.5

### Major Changes

- 9fbb74ec: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- 4cc0ead2: Update replace-string to v3.1.0.
- Updated dependencies [da091c71]
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/core-loggers@4.0.2-alpha.0
  - dependency-path@4.0.7-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.0
  - @pnpm/npm-resolver@7.3.12-alpha.2
  - @pnpm/package-is-installable@4.0.8-alpha.0
  - @pnpm/pick-registry-for-package@1.0.1-alpha.0
  - @pnpm/resolver-base@7.0.1-alpha.0

## 14.4.5-alpha.4

### Patch Changes

- 0730bb938: Check the existense of a dependency in `node_modules` at the right location.

## 14.4.5-alpha.3

### Patch Changes

- Updated dependencies [5bc033c43]
  - @pnpm/npm-resolver@8.0.0-alpha.1

## 14.4.5-alpha.2

### Patch Changes

- Updated dependencies [42e6490d1]
- Updated dependencies [f453a5f46]
  - @pnpm/store-controller-types@8.0.0-alpha.2
  - @pnpm/npm-resolver@7.3.12-alpha.0

## 14.4.5-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/store-controller-types@8.0.0-alpha.1

## 14.4.5-alpha.0

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/store-controller-types@8.0.0-alpha.0

## 14.4.4

### Patch Changes

- Updated dependencies [907c63a48]
  - @pnpm/lockfile-utils@2.0.11
