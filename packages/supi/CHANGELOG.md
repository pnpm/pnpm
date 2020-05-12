# supi

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
