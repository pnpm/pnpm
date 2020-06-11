# @pnpm/plugin-commands-installation

## 2.0.13

### Patch Changes

- Updated dependencies [e37a5a175]
  - @pnpm/filter-workspace-packages@2.1.0
  - @pnpm/plugin-commands-rebuild@2.0.9

## 2.0.12

### Patch Changes

- e934b1a48: Update chalk to v4.1.0.
- Updated dependencies [41d92948b]
- Updated dependencies [e934b1a48]
  - supi@0.40.0
  - @pnpm/cli-utils@0.4.8
  - @pnpm/pnpmfile@0.1.11
  - @pnpm/outdated@7.0.27
  - @pnpm/plugin-commands-rebuild@2.0.9
  - @pnpm/store-connection-manager@0.3.7
  - @pnpm/find-workspace-packages@2.2.5
  - @pnpm/filter-workspace-packages@2.0.18
  - @pnpm/package-store@9.0.5

## 2.0.11

### Patch Changes

- d3ddd023c: Update p-limit to v3.
- Updated dependencies [0e7ec4533]
- Updated dependencies [13630c659]
- Updated dependencies [d3ddd023c]
  - supi@0.39.10
  - @pnpm/package-store@9.0.4
  - @pnpm/plugin-commands-rebuild@2.0.8
  - @pnpm/store-connection-manager@0.3.6
  - @pnpm/pnpmfile@0.1.10
  - @pnpm/outdated@7.0.26
  - @pnpm/cli-utils@0.4.7
  - @pnpm/find-workspace-packages@2.2.4
  - @pnpm/filter-workspace-packages@2.0.17

## 2.0.10

### Patch Changes

- @pnpm/package-store@9.0.3
- supi@0.39.9
- @pnpm/store-connection-manager@0.3.5
- @pnpm/plugin-commands-rebuild@2.0.7

## 2.0.9

### Patch Changes

- @pnpm/package-store@9.0.2
- @pnpm/outdated@7.0.25
- @pnpm/store-connection-manager@0.3.4
- supi@0.39.8
- @pnpm/plugin-commands-rebuild@2.0.6

## 2.0.8

### Patch Changes

- @pnpm/store-connection-manager@0.3.3
- @pnpm/plugin-commands-rebuild@2.0.5

## 2.0.7

### Patch Changes

- Updated dependencies [ffddf34a8]
- Updated dependencies [ffddf34a8]
- Updated dependencies [429c5a560]
  - @pnpm/common-cli-options-help@0.2.0
  - @pnpm/config@9.1.0
  - @pnpm/package-store@9.0.1
  - @pnpm/plugin-commands-rebuild@2.0.4
  - @pnpm/cli-utils@0.4.6
  - @pnpm/find-workspace-packages@2.2.3
  - @pnpm/outdated@7.0.24
  - @pnpm/sort-packages@1.0.11
  - @pnpm/store-connection-manager@0.3.2
  - supi@0.39.7
  - @pnpm/filter-workspace-packages@2.0.16

## 2.0.6

### Patch Changes

- Updated dependencies [2f9c7ca85]
- Updated dependencies [160975d62]
  - supi@0.39.6

## 2.0.5

### Patch Changes

- @pnpm/store-connection-manager@0.3.1
- supi@0.39.5
- @pnpm/plugin-commands-rebuild@2.0.3

## 2.0.4

### Patch Changes

- @pnpm/plugin-commands-rebuild@2.0.2
- supi@0.39.4

## 2.0.3

### Patch Changes

- Updated dependencies [71b0cb8fd]
  - supi@0.39.3

## 2.0.2

### Patch Changes

- Updated dependencies [327bfbf02]
  - supi@0.39.2
  - @pnpm/plugin-commands-rebuild@2.0.1

## 2.0.1

### Patch Changes

- supi@0.39.1

## 2.0.0

### Major Changes

- da091c711: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- 9e2a5b827: `pnpm r` is not an alias of `pnpm remove`.
- e11019b89: Deprecate the resolution strategy setting. The fewer dependencies strategy is used always.
- 802d145fc: Remove `independent-leaves` support.
- 45fdcfde2: Locking is removed.

### Minor Changes

- 242cf8737: The `link-workspace-packages` setting may be set to `deep`. When using `deep`,
  workspace packages are linked into subdependencies, not only to direct dependencies of projects.
- f516d266c: Executables are saved into a separate directory inside the content-addressable storage.

### Patch Changes

- 083d78968: Allow referencing packages not under the directory containing `pnpm-workspace.yaml`.
- 6cbf18676: The add and install commands should accept setting the `modules-dir` setting.
- f453a5f46: Update version-selector-type to v3.
- Updated dependencies [b5f66c0f2]
- Updated dependencies [2e8ebabb2]
- Updated dependencies [242cf8737]
- Updated dependencies [ca9f50844]
- Updated dependencies [cc8a3bd31]
- Updated dependencies [cbc2192f1]
- Updated dependencies [3f73eaf0c]
- Updated dependencies [f516d266c]
- Updated dependencies [ecf2c6b7d]
- Updated dependencies [da091c711]
- Updated dependencies [a7d20d927]
- Updated dependencies [e11019b89]
- Updated dependencies [802d145fc]
- Updated dependencies [242cf8737]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [c207d994f]
- Updated dependencies [45fdcfde2]
- Updated dependencies [a5febb913]
- Updated dependencies [4f5801b1c]
- Updated dependencies [a5febb913]
- Updated dependencies [c25cccdad]
- Updated dependencies [f453a5f46]
- Updated dependencies [9fbb74ecb]
  - @pnpm/constants@4.0.0
  - @pnpm/package-store@9.0.0
  - @pnpm/plugin-commands-rebuild@2.0.0
  - supi@0.39.0
  - @pnpm/config@9.0.0
  - @pnpm/store-connection-manager@0.3.0
  - @pnpm/types@6.0.0
  - @pnpm/cli-utils@0.4.5
  - @pnpm/command@1.0.1
  - @pnpm/common-cli-options-help@0.1.6
  - @pnpm/error@1.2.1
  - @pnpm/filter-workspace-packages@2.0.15
  - @pnpm/find-workspace-dir@1.0.1
  - @pnpm/find-workspace-packages@2.2.2
  - @pnpm/manifest-utils@1.0.1
  - @pnpm/outdated@7.0.23
  - @pnpm/parse-wanted-dependency@1.0.1
  - @pnpm/pnpmfile@0.1.9
  - @pnpm/resolver-base@7.0.1
  - @pnpm/sort-packages@1.0.10

## 2.0.0-alpha.7

### Major Changes

- 45fdcfde2: Locking is removed.

### Minor Changes

- 242cf8737: The `link-workspace-packages` setting may be set to `deep`. When using `deep`,
  workspace packages are linked into subdependencies, not only to direct dependencies of projects.

### Patch Changes

- 083d78968: Allow referencing packages not under the directory containing `pnpm-workspace.yaml`.
- Updated dependencies [242cf8737]
- Updated dependencies [ca9f50844]
- Updated dependencies [cc8a3bd31]
- Updated dependencies [a7d20d927]
- Updated dependencies [242cf8737]
- Updated dependencies [45fdcfde2]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [c25cccdad]
  - @pnpm/config@9.0.0-alpha.2
  - @pnpm/constants@4.0.0-alpha.1
  - supi@0.39.0-alpha.7
  - @pnpm/package-store@9.0.0-alpha.5
  - @pnpm/plugin-commands-rebuild@2.0.0-alpha.5
  - @pnpm/store-connection-manager@0.3.0-alpha.5
  - @pnpm/cli-utils@0.4.5-alpha.2
  - @pnpm/find-workspace-packages@2.2.2-alpha.2
  - @pnpm/outdated@7.0.23-alpha.3
  - @pnpm/sort-packages@1.0.10-alpha.2
  - @pnpm/filter-workspace-packages@2.0.15-alpha.2

## 2.0.0-alpha.6

### Major Changes

- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.

### Patch Changes

- Updated dependencies [3f73eaf0]
- Updated dependencies [ecf2c6b7]
- Updated dependencies [da091c71]
- Updated dependencies [9fbb74ec]
  - @pnpm/plugin-commands-rebuild@2.0.0-alpha.4
  - supi@0.39.0-alpha.6
  - @pnpm/package-store@9.0.0-alpha.4
  - @pnpm/store-connection-manager@0.3.0-alpha.4
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/outdated@7.0.23-alpha.2
  - @pnpm/cli-utils@0.4.5-alpha.1
  - @pnpm/config@8.3.1-alpha.1
  - @pnpm/find-workspace-packages@2.2.2-alpha.1
  - @pnpm/manifest-utils@1.0.1-alpha.0
  - @pnpm/pnpmfile@0.1.9-alpha.0
  - @pnpm/resolver-base@7.0.1-alpha.0
  - @pnpm/sort-packages@1.0.10-alpha.1
  - @pnpm/filter-workspace-packages@2.0.15-alpha.1

## 2.0.0-alpha.5

### Patch Changes

- supi@0.38.30-alpha.5

## 2.0.0-alpha.4

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0
  - @pnpm/package-store@9.0.0-alpha.3
  - @pnpm/plugin-commands-rebuild@2.0.0-alpha.3
  - supi@0.39.0-alpha.4
  - @pnpm/config@8.3.1-alpha.0
  - @pnpm/find-workspace-packages@2.2.2-alpha.0
  - @pnpm/outdated@7.0.23-alpha.1
  - @pnpm/store-connection-manager@0.2.32-alpha.3
  - @pnpm/cli-utils@0.4.5-alpha.0
  - @pnpm/sort-packages@1.0.10-alpha.0
  - @pnpm/filter-workspace-packages@2.0.15-alpha.0

## 2.0.0-alpha.3

### Patch Changes

- f453a5f46: Update version-selector-type to v3.
- Updated dependencies [c207d994f]
- Updated dependencies [f453a5f46]
  - @pnpm/package-store@9.0.0-alpha.2
  - supi@0.39.0-alpha.3
  - @pnpm/plugin-commands-rebuild@1.0.11-alpha.2
  - @pnpm/store-connection-manager@0.2.32-alpha.2
  - @pnpm/outdated@7.0.23-alpha.0

## 2.0.0-alpha.2

### Major Changes

- 9e2a5b827: `pnpm r` is not an alias of `pnpm remove`.

### Patch Changes

- Updated dependencies [2e8ebabb2]
  - supi@0.39.0-alpha.2

## 1.3.0-alpha.1

### Minor Changes

- 4f62d0383: Executables are saved into a separate directory inside the content-addressable storage.

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/package-store@9.0.0-alpha.1
  - supi@0.39.0-alpha.1
  - @pnpm/store-connection-manager@0.2.32-alpha.1
  - @pnpm/plugin-commands-rebuild@1.0.11-alpha.1

## 1.2.4-alpha.0

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/package-store@9.0.0-alpha.0
  - @pnpm/store-connection-manager@0.3.0-alpha.0
  - supi@0.39.0-alpha.0
  - @pnpm/plugin-commands-rebuild@1.0.11-alpha.0

## 1.2.4

### Patch Changes

- Updated dependencies [760cc6664]
  - supi@0.38.30
  - @pnpm/plugin-commands-rebuild@1.0.11

## 1.2.3

### Patch Changes

- 907c63a48: Dependencies updated.
- 907c63a48: Dependencies updated.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - supi@0.38.29
  - @pnpm/outdated@7.0.22
  - @pnpm/store-connection-manager@0.2.31
  - @pnpm/package-store@8.1.0
  - @pnpm/plugin-commands-rebuild@1.0.10
  - @pnpm/filter-workspace-packages@2.0.14
  - @pnpm/cli-utils@0.4.4
  - @pnpm/find-workspace-packages@2.2.1
