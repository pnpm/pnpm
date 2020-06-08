# @pnpm/plugin-commands-rebuild

## 2.0.8

### Patch Changes

- d3ddd023c: Update p-limit to v3.
- Updated dependencies [d3ddd023c]
- Updated dependencies [2ebb7af33]
- Updated dependencies [68d8dc68f]
  - @pnpm/lifecycle@9.1.1
  - @pnpm/core-loggers@4.1.0
  - @pnpm/store-connection-manager@0.3.6
  - @pnpm/get-context@2.1.2
  - @pnpm/cli-utils@0.4.7
  - @pnpm/find-workspace-packages@2.2.4

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
- Updated dependencies [ffddf34a8]
- Updated dependencies [8094b2a62]
  - @pnpm/common-cli-options-help@0.2.0
  - @pnpm/config@9.1.0
  - @pnpm/lifecycle@9.1.0
  - @pnpm/cli-utils@0.4.6
  - @pnpm/find-workspace-packages@2.2.3
  - @pnpm/sort-packages@1.0.11
  - @pnpm/store-connection-manager@0.3.2

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
- 3f73eaf0c: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.
- 802d145fc: Remove `independent-leaves` support.
- 9fbb74ecb: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [b5f66c0f2]
- Updated dependencies [242cf8737]
- Updated dependencies [ca9f50844]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [3f73eaf0c]
- Updated dependencies [f516d266c]
- Updated dependencies [142f8caf7]
- Updated dependencies [da091c711]
- Updated dependencies [f35a3ec1c]
- Updated dependencies [42e6490d1]
- Updated dependencies [e11019b89]
- Updated dependencies [a5febb913]
- Updated dependencies [802d145fc]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [a5febb913]
- Updated dependencies [45fdcfde2]
- Updated dependencies [a5febb913]
- Updated dependencies [4f5801b1c]
- Updated dependencies [a5febb913]
- Updated dependencies [e3990787a]
  - @pnpm/constants@4.0.0
  - @pnpm/config@9.0.0
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/get-context@2.0.0
  - @pnpm/modules-yaml@7.0.0
  - @pnpm/lockfile-walker@3.0.0
  - @pnpm/store-connection-manager@0.3.0
  - @pnpm/types@6.0.0
  - @pnpm/lifecycle@9.0.0
  - @pnpm/cli-utils@0.4.5
  - @pnpm/common-cli-options-help@0.1.6
  - @pnpm/core-loggers@4.0.2
  - dependency-path@4.0.7
  - @pnpm/find-workspace-packages@2.2.2
  - @pnpm/link-bins@5.3.3
  - @pnpm/lockfile-utils@2.0.12
  - @pnpm/normalize-registries@1.0.1
  - @pnpm/sort-packages@1.0.10

## 2.0.0-alpha.5

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [242cf8737]
- Updated dependencies [ca9f50844]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [45fdcfde2]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/config@9.0.0-alpha.2
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/store-controller-types@8.0.0-alpha.4
  - @pnpm/store-connection-manager@0.3.0-alpha.5
  - @pnpm/cli-utils@0.4.5-alpha.2
  - @pnpm/find-workspace-packages@2.2.2-alpha.2
  - @pnpm/sort-packages@1.0.10-alpha.2
  - @pnpm/get-context@1.2.2-alpha.2
  - @pnpm/lockfile-utils@2.0.12-alpha.1
  - @pnpm/lockfile-walker@2.0.3-alpha.1

## 2.0.0-alpha.4

### Major Changes

- 3f73eaf0: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.
- 9fbb74ec: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- Updated dependencies [3f73eaf0]
- Updated dependencies [da091c71]
- Updated dependencies [e3990787]
  - @pnpm/get-context@2.0.0-alpha.1
  - @pnpm/modules-yaml@7.0.0-alpha.0
  - @pnpm/store-connection-manager@0.3.0-alpha.4
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/lifecycle@9.0.0-alpha.1
  - @pnpm/cli-utils@0.4.5-alpha.1
  - @pnpm/config@8.3.1-alpha.1
  - @pnpm/core-loggers@4.0.2-alpha.0
  - dependency-path@4.0.7-alpha.0
  - @pnpm/find-workspace-packages@2.2.2-alpha.1
  - @pnpm/link-bins@5.3.3-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.0
  - @pnpm/lockfile-walker@2.0.3-alpha.0
  - @pnpm/normalize-registries@1.0.1-alpha.0
  - @pnpm/sort-packages@1.0.10-alpha.1

## 2.0.0-alpha.3

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0
  - @pnpm/config@8.3.1-alpha.0
  - @pnpm/find-workspace-packages@2.2.2-alpha.0
  - @pnpm/get-context@1.2.2-alpha.0
  - @pnpm/store-connection-manager@0.2.32-alpha.3
  - @pnpm/cli-utils@0.4.5-alpha.0
  - @pnpm/sort-packages@1.0.10-alpha.0

## 1.0.11-alpha.2

### Patch Changes

- Updated dependencies [f35a3ec1c]
- Updated dependencies [42e6490d1]
  - @pnpm/lifecycle@8.2.0-alpha.0
  - @pnpm/store-controller-types@8.0.0-alpha.2
  - @pnpm/store-connection-manager@0.2.32-alpha.2

## 1.0.11-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/store-controller-types@8.0.0-alpha.1
  - @pnpm/store-connection-manager@0.2.32-alpha.1

## 1.0.11-alpha.0

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/store-controller-types@8.0.0-alpha.0
  - @pnpm/store-connection-manager@0.3.0-alpha.0

## 1.0.11

### Patch Changes

- Updated dependencies [2ec4c4eb9]
  - @pnpm/lifecycle@8.2.0

## 1.0.10

### Patch Changes

- 907c63a48: Dependencies updated.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/store-connection-manager@0.2.31
  - @pnpm/link-bins@5.3.2
  - @pnpm/get-context@1.2.1
  - @pnpm/lockfile-utils@2.0.11
  - @pnpm/modules-yaml@6.0.2
  - @pnpm/cli-utils@0.4.4
  - @pnpm/find-workspace-packages@2.2.1
