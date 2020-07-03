# supi

## 0.41.6

### Patch Changes

- @pnpm/headless@14.0.6
- @pnpm/package-requester@12.0.6
- @pnpm/resolve-dependencies@16.0.3

## 0.41.5

### Patch Changes

- @pnpm/package-requester@12.0.6
- @pnpm/headless@14.0.5

## 0.41.4

### Patch Changes

- 220896511: Remove common-tags from dependencies.
- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/build-modules@5.0.6
  - @pnpm/core-loggers@4.1.2
  - dependency-path@5.0.2
  - @pnpm/filter-lockfile@4.0.4
  - @pnpm/get-context@3.0.1
  - @pnpm/headless@14.0.4
  - @pnpm/hoist@4.0.3
  - @pnpm/lifecycle@9.1.3
  - @pnpm/link-bins@5.3.7
  - @pnpm/lockfile-file@3.0.11
  - @pnpm/lockfile-utils@2.0.15
  - @pnpm/lockfile-walker@3.0.3
  - @pnpm/manifest-utils@1.0.3
  - @pnpm/modules-cleaner@10.0.3
  - @pnpm/modules-yaml@8.0.1
  - @pnpm/normalize-registries@1.0.3
  - @pnpm/package-requester@12.0.6
  - @pnpm/prune-lockfile@2.0.11
  - @pnpm/read-package-json@3.1.3
  - @pnpm/read-project-manifest@1.0.9
  - @pnpm/remove-bins@1.0.1
  - @pnpm/resolve-dependencies@16.0.2
  - @pnpm/resolver-base@7.0.3
  - @pnpm/store-controller-types@8.0.2
  - @pnpm/symlink-dependency@3.0.8

## 0.41.3

### Patch Changes

- 57d08f303: Remove global bins when unlinking.
- Updated dependencies [57d08f303]
  - @pnpm/remove-bins@1.0.0
  - @pnpm/modules-cleaner@10.0.2
  - @pnpm/headless@14.0.3

## 0.41.2

### Patch Changes

- 17b598c18: Don't remove skipped optional dependencies from the current lockfile on partial installation.
- 1520e3d6f: Update graceful-fs to v4.2.4
  - @pnpm/package-requester@12.0.5
  - @pnpm/link-bins@5.3.6
  - @pnpm/modules-cleaner@10.0.1
  - @pnpm/headless@14.0.2
  - @pnpm/build-modules@5.0.5
  - @pnpm/hoist@4.0.2

## 0.41.1

### Patch Changes

- Updated dependencies [0a2f3ecc6]
  - @pnpm/hoist@4.0.1
  - @pnpm/headless@14.0.1

## 0.41.0

### Minor Changes

- 71a8c8ce3: `shamefullyHoist` replaced by `publicHoistPattern` and `forcePublicHoistPattern`.
- 71a8c8ce3: Breaking changes to the `node_modules/.modules.yaml` file:
  - `hoistedAliases` replaced with `hoistedDependencies`.
  - `shamefullyHoist` replaced with `publicHoistPattern`.

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [e1ca9fc13]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/hoist@4.0.0
  - @pnpm/link-bins@5.3.5
  - @pnpm/modules-cleaner@10.0.0
  - @pnpm/headless@14.0.0
  - @pnpm/get-context@3.0.0
  - @pnpm/modules-yaml@8.0.0
  - @pnpm/build-modules@5.0.4
  - @pnpm/core-loggers@4.1.1
  - dependency-path@5.0.1
  - @pnpm/filter-lockfile@4.0.3
  - @pnpm/lifecycle@9.1.2
  - @pnpm/lockfile-file@3.0.10
  - @pnpm/lockfile-utils@2.0.14
  - @pnpm/lockfile-walker@3.0.2
  - @pnpm/manifest-utils@1.0.2
  - @pnpm/normalize-registries@1.0.2
  - @pnpm/package-requester@12.0.5
  - @pnpm/prune-lockfile@2.0.10
  - @pnpm/read-package-json@3.1.2
  - @pnpm/read-project-manifest@1.0.8
  - @pnpm/resolve-dependencies@16.0.1
  - @pnpm/resolver-base@7.0.2
  - @pnpm/store-controller-types@8.0.1
  - @pnpm/symlink-dependency@3.0.7

## 0.40.1

### Patch Changes

- @pnpm/package-requester@12.0.4
- @pnpm/headless@13.0.6

## 0.40.0

### Minor Changes

- 41d92948b: It should be possible to install a tarball through a non-standard URL endpoint served via the registry domain.

  For instance, the configured registry is `https://registry.npm.taobao.org/`.
  It should be possible to run `pnpm add https://registry.npm.taobao.org/vue/download/vue-2.0.0.tgz`

### Patch Changes

- Updated dependencies [41d92948b]
- Updated dependencies [57c510f00]
- Updated dependencies [41d92948b]
  - dependency-path@5.0.0
  - @pnpm/read-project-manifest@1.0.7
  - @pnpm/resolve-dependencies@16.0.0
  - @pnpm/filter-lockfile@4.0.2
  - @pnpm/headless@13.0.5
  - @pnpm/hoist@3.0.2
  - @pnpm/lockfile-utils@2.0.13
  - @pnpm/lockfile-walker@3.0.1
  - @pnpm/modules-cleaner@9.0.2
  - @pnpm/prune-lockfile@2.0.9
  - @pnpm/link-bins@5.3.4
  - @pnpm/build-modules@5.0.3
  - @pnpm/package-requester@12.0.3

## 0.39.10

### Patch Changes

- 0e7ec4533: Remove @pnpm/check-package from dependencies.
- 13630c659: Perform headless installation when dependencies should not be linked from the workspace, and they are not indeed linked from the workspace.
- d3ddd023c: Update p-limit to v3.
- Updated dependencies [d3ddd023c]
- Updated dependencies [2ebb7af33]
- Updated dependencies [68d8dc68f]
  - @pnpm/build-modules@5.0.2
  - @pnpm/headless@13.0.4
  - @pnpm/lifecycle@9.1.1
  - @pnpm/package-requester@12.0.3
  - @pnpm/core-loggers@4.1.0
  - @pnpm/resolve-dependencies@15.1.2
  - @pnpm/get-context@2.1.2
  - @pnpm/modules-cleaner@9.0.1
  - @pnpm/symlink-dependency@3.0.6
  - @pnpm/filter-lockfile@4.0.1
  - @pnpm/hoist@3.0.1

## 0.39.9

### Patch Changes

- Updated dependencies [a203bc138]
  - @pnpm/package-requester@12.0.2
  - @pnpm/headless@13.0.3

## 0.39.8

### Patch Changes

- @pnpm/package-requester@12.0.1
- @pnpm/resolve-dependencies@15.1.1
- @pnpm/headless@13.0.2

## 0.39.7

### Patch Changes

- Updated dependencies [8094b2a62]
  - @pnpm/lifecycle@9.1.0
  - @pnpm/package-requester@12.0.1
  - @pnpm/build-modules@5.0.1
  - @pnpm/headless@13.0.1

## 0.39.6

### Patch Changes

- 2f9c7ca85: Fix a regression introduced in pnpm v5.0.0.
  Create correct lockfile when the package tarball is hosted not under the registry domain.
- 160975d62: This fixes a regression introduced in pnpm v5.0.0. Direct local tarball dependencies should always be reanalized on install.

## 0.39.5

### Patch Changes

- @pnpm/headless@13.0.0

## 0.39.4

### Patch Changes

- Updated dependencies [58c02009f]
  - @pnpm/get-context@2.1.1

## 0.39.3

### Patch Changes

- 71b0cb8fd: Subdependencies are not needlessly updated.

  Fixes a regression introduced by [cc8a3bd312ea1405a6c79b1d157f0f9ae1be07aa](https://github.com/pnpm/pnpm/commit/cc8a3bd312ea1405a6c79b1d157f0f9ae1be07aa).

- Updated dependencies [71b0cb8fd]
  - @pnpm/resolve-dependencies@15.1.0

## 0.39.2

### Patch Changes

- 327bfbf02: Fix current lockfile (the one at `node_modules/.pnpm/lock.yaml`) up-to-date check.
- Updated dependencies [327bfbf02]
  - @pnpm/get-context@2.1.0

## 0.39.1

### Patch Changes

- Updated dependencies [e2c4fdad5]
  - @pnpm/resolve-dependencies@15.0.1

## 0.39.0

### Minor Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- 3f73eaf0c: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.
- f516d266c: Executables are saved into a separate directory inside the content-addressable storage.
- da091c711: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- e11019b89: Deprecate the resolution strategy setting. The fewer dependencies strategy is used always.
- 802d145fc: Remove `independent-leaves` support.
- 242cf8737: The `linkWorkspacePackages` option is removed. A new option called `linkWorkspacePackagesDepth` is added.
  When `linkWorkspacePackageDepth` is `0`, workspace packages are linked to direct dependencies even if these direct
  dependencies are not using workspace ranges (so this is similar to the old `linkWorkspacePackages=true`).
  `linkWorkspacePackageDepth` also allows to link workspace packages to subdependencies by setting the max depth.
  Setting it to `Infinity` will make the resolution algorithm always prefer packages from the workspace over packages
  from the registry.
- b6a82072e: Using a content-addressable filesystem for storing packages.
- 45fdcfde2: Locking is removed.
- a5febb913: The importPackage function of the store controller is importing packages directly from the side-effects cache.
- 9fbb74ecb: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- 2e8ebabb2: Headless installation should be preferred when local dependencies that use aliases are up-to-date.
- cc8a3bd31: Installation on a non-up-to-date `node_modules`.
- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- c25cccdad: The lockfile should be recreated correctly when an up-to-date `node_modules` is present.
  The recreated lockfile should contain all the skipped optional dependencies.
- f453a5f46: Update version-selector-type to v3.
- Updated dependencies [b5f66c0f2]
- Updated dependencies [0730bb938]
- Updated dependencies [ca9f50844]
- Updated dependencies [9596774f2]
- Updated dependencies [7179cc560]
- Updated dependencies [77bc9b510]
- Updated dependencies [c25cccdad]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [242cf8737]
- Updated dependencies [3f73eaf0c]
- Updated dependencies [f516d266c]
- Updated dependencies [cc8a3bd31]
- Updated dependencies [142f8caf7]
- Updated dependencies [da091c711]
- Updated dependencies [9b1b520d9]
- Updated dependencies [f35a3ec1c]
- Updated dependencies [a7d20d927]
- Updated dependencies [42e6490d1]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [2485eaf60]
- Updated dependencies [64bae33c4]
- Updated dependencies [e11019b89]
- Updated dependencies [a5febb913]
- Updated dependencies [bb59db642]
- Updated dependencies [b47f9737a]
- Updated dependencies [802d145fc]
- Updated dependencies [f93583d52]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [a5febb913]
- Updated dependencies [c207d994f]
- Updated dependencies [a5febb913]
- Updated dependencies [4f5801b1c]
- Updated dependencies [a5febb913]
- Updated dependencies [4cc0ead24]
- Updated dependencies [471149e66]
- Updated dependencies [c25cccdad]
- Updated dependencies [42e6490d1]
- Updated dependencies [9fbb74ecb]
- Updated dependencies [e3990787a]
  - @pnpm/constants@4.0.0
  - @pnpm/headless@13.0.0
  - @pnpm/hoist@3.0.0
  - @pnpm/modules-cleaner@9.0.0
  - @pnpm/package-requester@12.0.0
  - @pnpm/resolve-dependencies@15.0.0
  - @pnpm/filter-lockfile@4.0.0
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/get-context@2.0.0
  - @pnpm/modules-yaml@7.0.0
  - @pnpm/lockfile-walker@3.0.0
  - @pnpm/types@6.0.0
  - @pnpm/build-modules@5.0.0
  - @pnpm/lifecycle@9.0.0
  - @pnpm/core-loggers@4.0.2
  - dependency-path@4.0.7
  - @pnpm/error@1.2.1
  - @pnpm/link-bins@5.3.3
  - @pnpm/lockfile-file@3.0.9
  - @pnpm/lockfile-utils@2.0.12
  - @pnpm/manifest-utils@1.0.1
  - @pnpm/matcher@1.0.3
  - @pnpm/normalize-registries@1.0.1
  - @pnpm/parse-wanted-dependency@1.0.1
  - @pnpm/prune-lockfile@2.0.8
  - @pnpm/read-modules-dir@2.0.2
  - @pnpm/read-package-json@3.1.1
  - @pnpm/read-project-manifest@1.0.6
  - @pnpm/resolver-base@7.0.1
  - @pnpm/symlink-dependency@3.0.5

## 0.39.0-alpha.7

### Minor Changes

- 242cf8737: The `linkWorkspacePackages` option is removed. A new option called `linkWorkspacePackagesDepth` is added.
  When `linkWorkspacePackageDepth` is `0`, workspace packages are linked to direct dependencies even if these direct
  dependencies are not using workspace ranges (so this is similar to the old `linkWorkspacePackages=true`).
  `linkWorkspacePackageDepth` also allows to link workspace packages to subdependencies by setting the max depth.
  Setting it to `Infinity` will make the resolution algorithm always prefer packages from the workspace over packages
  from the registry.
- 45fdcfde2: Locking is removed.
- a5febb913: The importPackage function of the store controller is importing packages directly from the side-effects cache.

### Patch Changes

- cc8a3bd31: Installation on a non-up-to-date `node_modules`.
- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- c25cccdad: The lockfile should be recreated correctly when an up-to-date `node_modules` is present.
  The recreated lockfile should contain all the skipped optional dependencies.
- Updated dependencies [ca9f50844]
- Updated dependencies [c25cccdad]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [242cf8737]
- Updated dependencies [cc8a3bd31]
- Updated dependencies [a7d20d927]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [2485eaf60]
- Updated dependencies [a5febb913]
- Updated dependencies [b47f9737a]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [c25cccdad]
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/filter-lockfile@4.0.0-alpha.2
  - @pnpm/package-requester@12.0.0-alpha.5
  - @pnpm/store-controller-types@8.0.0-alpha.4
  - @pnpm/resolve-dependencies@15.0.0-alpha.6
  - @pnpm/headless@13.0.0-alpha.5
  - @pnpm/hoist@3.0.0-alpha.2
  - @pnpm/modules-cleaner@9.0.0-alpha.5
  - @pnpm/build-modules@5.0.0-alpha.5
  - @pnpm/get-context@1.2.2-alpha.2
  - @pnpm/lockfile-file@3.0.9-alpha.2
  - @pnpm/prune-lockfile@2.0.8-alpha.2
  - @pnpm/lockfile-utils@2.0.12-alpha.1
  - @pnpm/lockfile-walker@2.0.3-alpha.1

## 0.39.0-alpha.6

### Minor Changes

- 3f73eaf0: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.
- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- 9fbb74ec: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- Updated dependencies [7179cc56]
- Updated dependencies [3f73eaf0]
- Updated dependencies [da091c71]
- Updated dependencies [4cc0ead2]
- Updated dependencies [471149e6]
- Updated dependencies [9fbb74ec]
- Updated dependencies [e3990787]
  - @pnpm/modules-cleaner@9.0.0-alpha.4
  - @pnpm/get-context@2.0.0-alpha.1
  - @pnpm/headless@13.0.0-alpha.4
  - @pnpm/modules-yaml@7.0.0-alpha.0
  - @pnpm/package-requester@12.0.0-alpha.4
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/resolve-dependencies@15.0.0-alpha.5
  - @pnpm/hoist@3.0.0-alpha.1
  - @pnpm/build-modules@5.0.0-alpha.4
  - @pnpm/lifecycle@9.0.0-alpha.1
  - @pnpm/core-loggers@4.0.2-alpha.0
  - dependency-path@4.0.7-alpha.0
  - @pnpm/filter-lockfile@3.2.3-alpha.1
  - @pnpm/link-bins@5.3.3-alpha.0
  - @pnpm/lockfile-file@3.0.9-alpha.1
  - @pnpm/lockfile-utils@2.0.12-alpha.0
  - @pnpm/lockfile-walker@2.0.3-alpha.0
  - @pnpm/manifest-utils@1.0.1-alpha.0
  - @pnpm/normalize-registries@1.0.1-alpha.0
  - @pnpm/prune-lockfile@2.0.8-alpha.1
  - @pnpm/read-package-json@3.1.1-alpha.0
  - @pnpm/read-project-manifest@1.0.6-alpha.0
  - @pnpm/resolver-base@7.0.1-alpha.0
  - @pnpm/symlink-dependency@3.0.5-alpha.0

## 0.39.0-alpha.5

### Patch Changes

- Updated dependencies [0730bb938]
  - @pnpm/resolve-dependencies@14.4.5-alpha.4

## 0.39.0-alpha.4

### Minor Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [9596774f2]
  - @pnpm/constants@4.0.0-alpha.0
  - @pnpm/headless@13.0.0-alpha.3
  - @pnpm/hoist@3.0.0-alpha.0
  - @pnpm/modules-cleaner@9.0.0-alpha.3
  - @pnpm/package-requester@12.0.0-alpha.3
  - @pnpm/build-modules@4.1.15-alpha.3
  - @pnpm/filter-lockfile@3.2.3-alpha.0
  - @pnpm/get-context@1.2.2-alpha.0
  - @pnpm/lockfile-file@3.0.9-alpha.0
  - @pnpm/prune-lockfile@2.0.8-alpha.0
  - @pnpm/resolve-dependencies@14.4.5-alpha.3

## 0.39.0-alpha.3

### Patch Changes

- f453a5f46: Update version-selector-type to v3.
- Updated dependencies [f35a3ec1c]
- Updated dependencies [42e6490d1]
- Updated dependencies [64bae33c4]
- Updated dependencies [c207d994f]
- Updated dependencies [42e6490d1]
  - @pnpm/lifecycle@8.2.0-alpha.0
  - @pnpm/package-requester@12.0.0-alpha.2
  - @pnpm/store-controller-types@8.0.0-alpha.2
  - @pnpm/build-modules@4.1.14-alpha.2
  - @pnpm/headless@12.2.2-alpha.2
  - @pnpm/modules-cleaner@8.0.17-alpha.2
  - @pnpm/resolve-dependencies@14.4.5-alpha.2

## 0.39.0-alpha.2

### Patch Changes

- 2e8ebabb2: Headless installation should be preferred when local dependencies that use aliases are up-to-date.

## 0.39.0-alpha.1

### Minor Changes

- 4f62d0383: Executables are saved into a separate directory inside the content-addressable storage.

### Patch Changes

- Updated dependencies [4f62d0383]
- Updated dependencies [f93583d52]
  - @pnpm/package-requester@12.0.0-alpha.1
  - @pnpm/store-controller-types@8.0.0-alpha.1
  - @pnpm/headless@12.2.2-alpha.1
  - @pnpm/build-modules@4.1.14-alpha.1
  - @pnpm/modules-cleaner@8.0.17-alpha.1
  - @pnpm/resolve-dependencies@14.4.5-alpha.1

## 0.39.0-alpha.0

### Minor Changes

- 91c4b5954: Using a content-addressable filesystem for storing packages.

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/headless@13.0.0-alpha.0
  - @pnpm/package-requester@12.0.0-alpha.0
  - @pnpm/store-controller-types@8.0.0-alpha.0
  - @pnpm/build-modules@4.1.14-alpha.0
  - @pnpm/modules-cleaner@8.0.17-alpha.0
  - @pnpm/resolve-dependencies@14.4.5-alpha.0

## 0.38.30

### Patch Changes

- 760cc6664: Headless installation should be preferred when local dependencies that use aliases are up-to-date.
- Updated dependencies [2ec4c4eb9]
  - @pnpm/lifecycle@8.2.0
  - @pnpm/build-modules@4.1.14
  - @pnpm/headless@12.2.2

## 0.38.29

### Patch Changes

- 907c63a48: Update symlink-dir to v4.
- 907c63a48: Update `@pnpm/store-path`.
- 907c63a48: Dependencies updated.
- 907c63a48: Dependencies updated.
- 907c63a48: Use `fs.mkdir` instead of `make-dir`.
- 907c63a48: `pnpm update --no-save` does not update the specs in the `package.json` files.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/package-requester@11.0.6
  - @pnpm/symlink-dependency@3.0.4
  - @pnpm/headless@12.2.1
  - @pnpm/link-bins@5.3.2
  - @pnpm/lockfile-file@3.0.8
  - @pnpm/matcher@1.0.2
  - @pnpm/get-context@1.2.1
  - @pnpm/filter-lockfile@3.2.2
  - @pnpm/lockfile-utils@2.0.11
  - @pnpm/modules-yaml@6.0.2
  - @pnpm/hoist@2.2.3
  - @pnpm/build-modules@4.1.13
  - @pnpm/modules-cleaner@8.0.16
  - @pnpm/resolve-dependencies@14.4.4
  - @pnpm/read-project-manifest@1.0.5
